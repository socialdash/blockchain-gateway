mod responses;

use std::sync::Arc;

use hyper::{Body, Request};

use self::responses::*;
use super::error::*;
use super::http_client::HttpClient;
use config::Mode;
use futures::{future, stream};
use models::*;
use prelude::*;
use serde_json;
use utils::read_body;

pub trait EthereumClient: Send + Sync + 'static {
    /// Get account nonce (needed for creating transactions)
    fn get_nonce(&self, address: EthereumAddress) -> Box<Future<Item = u64, Error = Error> + Send>;
    /// Send raw eth/stq transaction to blockchain
    fn send_raw_tx(&self, tx: RawEthereumTransaction) -> Box<Future<Item = TxHash, Error = Error> + Send>;
    /// Get transaction by hash. Since getting block_number from transaction is not yet
    /// supported, you need to provide one in arguments
    fn get_eth_transaction(&self, hash: String) -> Box<Future<Item = BlockchainTransaction, Error = Error> + Send>;
    /// Get transactions from blocks starting from `start_block_hash` (or the most recent block if not specified)
    /// and fetch previous blocks. Total number of blocks = `blocks_count`.
    /// `blocks_count` should be greater than 0.
    fn last_eth_transactions(
        &self,
        start_block_hash: Option<String>,
        blocks_count: u64,
    ) -> Box<Stream<Item = BlockchainTransaction, Error = Error> + Send>;
    /// Same as `get_eth_transaction` for stq. Since there could be many stq transfers in one transaction
    /// we return Stream here.
    fn get_stq_transactions(&self, hash: String) -> Box<Stream<Item = BlockchainTransaction, Error = Error> + Send>;
    /// Same as `last_eth_transactions` for stq
    fn last_stq_transactions(
        &self,
        start_block_hash: Option<String>,
        blocks_count: u64,
    ) -> Box<Stream<Item = BlockchainTransaction, Error = Error> + Send>;
}

const ADDRESS_LENGTH: usize = 40;

#[derive(Clone)]
pub struct EthereumClientImpl {
    http_client: Arc<HttpClient>,
    infura_url: String,
    stq_contract_address: String,
    stq_transfer_topic: String,
}

impl EthereumClientImpl {
    pub fn new(
        http_client: Arc<HttpClient>,
        mode: Mode,
        api_key: String,
        stq_contract_address: String,
        stq_transfer_topic: String,
    ) -> Self {
        let infura_url = match mode {
            Mode::Production => format!("https://mainnet.infura.io/{}", api_key),
            _ => format!("https://kovan.infura.io/{}", api_key),
        };
        Self {
            http_client,
            infura_url,
            stq_contract_address,
            stq_transfer_topic,
        }
    }
}

impl EthereumClientImpl {
    // Eth

    /// Gets NON-ZERO value eth transactions (that means that ERC-20 are not here)
    fn get_eth_transactions_for_block(&self, block: u64) -> impl Stream<Item = PartialBlockchainTransaction, Error = Error> + Send {
        let block = format!("0x{:x}", block);
        let params = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_getBlockByNumber",
            "params": [block, true]
        });
        self.get_rpc_response::<BlockByNumberResponse>(&params)
            .into_stream()
            .map(|resp| {
                stream::iter_result(
                    resp.result
                        .unwrap_or(Default::default())
                        .transactions
                        .into_iter()
                        .map(|tx_resp| EthereumClientImpl::eth_response_to_partial_tx(tx_resp.clone())),
                )
            }).flatten()
            .filter(|tx| tx.to[0].value.inner() > 0)
    }

    fn last_eth_transactions_with_current_block(
        &self,
        start_block_hash: Option<String>,
        blocks_count: u64,
        current_block: u64,
    ) -> impl Stream<Item = BlockchainTransaction, Error = Error> + Send {
        let to_block_number_f = match start_block_hash {
            Some(hash) => future::Either::A(self.get_block_number_by_hash(hash)),
            None => future::Either::B(Ok(current_block).into_future()),
        };
        let self_clone = self.clone();
        let self_clone2 = self.clone();
        to_block_number_f
            .into_stream()
            .map(move |to_block| stream::iter_ok::<_, Error>(to_block - blocks_count + 1..=to_block))
            .flatten()
            .map(move |block_number| self_clone.get_eth_transactions_for_block(block_number))
            .flatten()
            .and_then(move |tx| self_clone2.partial_tx_to_tx(&tx, current_block))
    }

    fn get_eth_partial_transaction(&self, hash: String) -> impl Future<Item = PartialBlockchainTransaction, Error = Error> + Send {
        let hash = format!("0x{}", hash);
        let params = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_getTransactionByHash",
            "params": [hash]
        });
        self.get_rpc_response::<TransactionByHashResponse>(&params)
            .and_then(|resp| EthereumClientImpl::eth_response_to_partial_tx(resp.result))
    }

    fn eth_response_to_partial_tx(resp: TransactionResponse) -> Result<PartialBlockchainTransaction, Error> {
        let TransactionResponse {
            block_number,
            hash,
            from,
            to,
            value,
            gas_price,
        } = resp;
        let block_number = EthereumClientImpl::parse_hex(block_number)? as u64;
        let value = Amount::new(EthereumClientImpl::parse_hex(value)?);
        let gas_price = Amount::new(EthereumClientImpl::parse_hex(gas_price)?);
        let from = vec![(&from[2..]).to_string()];
        let to_address = to.map(|t| (&t[2..]).to_string()).unwrap_or("0".to_string());
        let to = vec![BlockchainTransactionEntry {
            address: to_address,
            value,
        }];
        Ok(PartialBlockchainTransaction {
            hash: (&hash[2..]).to_string(),
            from,
            to,
            block_number,
            currency: Currency::Eth,
            gas_price,
        })
    }

    // Stq

    fn get_stq_transactions_for_blocks(
        &self,
        from_block: u64,
        to_block: u64,
    ) -> impl Stream<Item = PartialBlockchainTransaction, Error = Error> + Send {
        let self_clone = self.clone();
        let from_block = format!("0x{:x}", from_block);
        let to_block = format!("0x{:x}", to_block);
        let params = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_getLogs",
            "params": [{
                "address": self.stq_contract_address,
                "topics": [self.stq_transfer_topic],
                "fromBlock": from_block,
                "toBlock": to_block,
            }]
        });
        self.get_rpc_response::<StqResponse>(&params)
            .into_stream()
            .map(|resp| stream::iter_ok(resp.result.unwrap_or(Default::default()).into_iter()))
            .flatten()
            .and_then(move |tx_resp| {
                self_clone
                    .get_eth_partial_transaction(tx_resp.transaction_hash.clone())
                    .map(|tx| (tx_resp, tx.gas_price))
            }).and_then(|(tx_resp, gas_price)| EthereumClientImpl::stq_response_to_partial_tx(tx_resp, gas_price))
    }

    fn last_stq_transactions_with_current_block(
        &self,
        start_block_hash: Option<String>,
        blocks_count: u64,
        current_block: u64,
    ) -> impl Stream<Item = BlockchainTransaction, Error = Error> + Send {
        let to_block_number_f = match start_block_hash {
            Some(hash) => future::Either::A(self.get_block_number_by_hash(hash)),
            None => future::Either::B(Ok(current_block).into_future()),
        };
        let self_clone = self.clone();
        let self_clone2 = self.clone();
        to_block_number_f
            .into_stream()
            .map(move |to_block| self_clone.get_stq_transactions_for_blocks(to_block - blocks_count + 1, to_block))
            .flatten()
            .and_then(move |tx| self_clone2.partial_tx_to_tx(&tx, current_block))
    }

    fn get_stq_transactions_with_current_block(
        &self,
        hash: String,
        current_block: u64,
    ) -> Box<Stream<Item = BlockchainTransaction, Error = Error> + Send> {
        let hash = format!("0x{}", hash);
        let params = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_getLogs",
            "params": [{
                "address": self.stq_contract_address,
                "topics": [self.stq_transfer_topic],
                "transactionHash": hash,
            }]
        });

        let self_clone = self.clone();
        let self_clone2 = self.clone();

        Box::new(
            self.get_rpc_response::<StqResponse>(&params)
                .into_stream()
                .map(|resp| stream::iter_ok(resp.result.unwrap_or(vec![]).into_iter()))
                .flatten()
                .and_then(move |tx_resp| {
                    self_clone
                        .get_eth_partial_transaction(tx_resp.transaction_hash.clone())
                        .map(|tx| (tx_resp, tx.gas_price))
                }).and_then(|(resp, gas_price)| EthereumClientImpl::stq_response_to_partial_tx(resp, gas_price))
                .and_then(move |partial_tx| self_clone2.partial_tx_to_tx(&partial_tx, current_block)),
        )
    }

    fn stq_response_to_partial_tx(log: StqResponseItem, gas_price: Amount) -> Result<PartialBlockchainTransaction, Error> {
        let from = log
            .topics
            .get(1)
            .map(|s| {
                let slice = &s[(s.len() - ADDRESS_LENGTH)..];
                slice.to_string()
            }).ok_or(ectx!(try err ErrorContext::Topics, ErrorKind::Internal))?;
        let to = log
            .topics
            .get(2)
            // remove 0x and leading zeroes
            .map(|s| {
                let slice = &s[(s.len() - ADDRESS_LENGTH)..];
                slice.to_string()
            })
            .ok_or(ectx!(try err ErrorContext::Topics, ErrorKind::Internal))?;
        let block_number = EthereumClientImpl::parse_hex(log.block_number).map(|x| x as u64)?;
        let value = EthereumClientImpl::parse_hex(log.data).map(Amount::new)?;
        let log_index = EthereumClientImpl::parse_hex(log.log_index)?;
        // Since there can be many ERC-20 transfers per ETH transaction, we're giving extended hash here
        let hash = format!("{}:{}", log.transaction_hash[2..].to_string(), log_index);
        let from = vec![from];
        let to = vec![BlockchainTransactionEntry { address: to, value }];
        Ok(PartialBlockchainTransaction {
            hash,
            from,
            to,
            block_number,
            currency: Currency::Stq,
            gas_price,
        })
    }

    // Common

    fn get_current_block_number(&self) -> impl Future<Item = u64, Error = Error> + Send {
        let params = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_blockNumber",
            "params": []
        });

        self.get_rpc_response::<NonceResponse>(&params).and_then(|resp| {
            u64::from_str_radix(&resp.result[2..], 16).map_err(ectx!(ErrorContext::Hex, ErrorKind::Internal => resp.result))
        })
    }

    fn get_block_number_by_hash(&self, hash: String) -> impl Future<Item = u64, Error = Error> + Send {
        let hash = format!("0x{}", hash);
        let params = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_getBlockByHash",
            "params": [hash, false]
        });

        self.get_rpc_response::<ShortBlockResponse>(&params)
            .and_then(|resp| EthereumClientImpl::parse_hex(resp.result.number))
            .map(|x| x as u64)
    }

    fn parse_hex(s: String) -> Result<u128, Error> {
        u128::from_str_radix(&s[2..], 16).map_err(ectx!(ErrorContext::Hex, ErrorKind::Internal => s))
    }

    fn get_rpc_response<T>(&self, params: &::serde_json::Value) -> impl Future<Item = T, Error = Error> + Send
    where
        for<'a> T: Send + 'static + ::serde::Deserialize<'a>,
    {
        let http_client = self.http_client.clone();
        let params_clone = params.clone();
        serde_json::to_string(params)
            .map_err(ectx!(ErrorContext::Json, ErrorKind::Internal => params))
            .and_then(|body| {
                Request::builder()
                .header("Content-Type", "application/json")
                .method("POST")
                .uri(self.infura_url.clone())
                .body(Body::from(body.clone()))
                .map_err(ectx!(ErrorSource::Hyper, ErrorKind::Internal => body))
            }).into_future()
            .and_then(move |request| http_client.request(request))
            .and_then(|resp| read_body(resp.into_body()).map_err(ectx!(ErrorKind::Internal => params_clone)))
            .and_then(|bytes| {
                let bytes_clone = bytes.clone();
                String::from_utf8(bytes).map_err(ectx!(ErrorContext::UTF8, ErrorKind::Internal => bytes_clone))
            }).and_then(|string| {
                serde_json::from_str::<T>(&string).map_err(ectx!(ErrorContext::Json, ErrorKind::Internal => string.clone()))
            })
    }

    fn partial_tx_to_tx(
        &self,
        tx: &PartialBlockchainTransaction,
        current_block: u64,
    ) -> impl Future<Item = BlockchainTransaction, Error = Error> {
        let hash = format!("0x{}", tx.hash);
        let params = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_getTransactionReceipt",
            "params": [hash]
        });
        let gas_price = tx.gas_price;
        let tx = tx.clone();
        self.get_rpc_response::<TransactionReceiptResponse>(&params).and_then(move |resp| {
            let resp_clone = resp.clone();
            let gas_used = EthereumClientImpl::parse_hex(resp.result.gas_used).map(Amount::new).into_future();
            let block_number = EthereumClientImpl::parse_hex(resp.result.block_number).into_future();
            gas_used.join(block_number).and_then(move |(gas_used, block_number)| {
                gas_used
                    .checked_mul(gas_price)
                    .ok_or(ectx!(err ErrorContext::Overflow, ErrorKind::Internal => resp_clone, gas_price))
                    .map(move |fee| {
                        let confirmations = (current_block as usize) - block_number as usize;
                        BlockchainTransaction {
                            hash: tx.hash,
                            from: tx.from,
                            to: tx.to,
                            block_number: tx.block_number,
                            currency: tx.currency,
                            fee,
                            confirmations,
                        }
                    })
            })
        })
    }
}

impl EthereumClient for EthereumClientImpl {
    fn last_eth_transactions(
        &self,
        start_block_hash: Option<String>,
        blocks_count: u64,
    ) -> Box<Stream<Item = BlockchainTransaction, Error = Error> + Send> {
        let self_clone = self.clone();
        Box::new(
            self.get_current_block_number()
                .into_stream()
                .map(move |current_block| {
                    let hash = start_block_hash.clone();
                    self_clone.last_eth_transactions_with_current_block(hash, blocks_count, current_block)
                }).flatten(),
        )
    }

    fn get_eth_transaction(&self, hash: String) -> Box<Future<Item = BlockchainTransaction, Error = Error> + Send> {
        let self_clone = self.clone();
        let f1 = self.get_current_block_number();
        let f2 = self.get_eth_partial_transaction(hash);
        Box::new(
            f1.join(f2)
                .and_then(move |(current_block, partial_tx)| self_clone.partial_tx_to_tx(&partial_tx, current_block)),
        )
    }

    fn last_stq_transactions(
        &self,
        start_block_hash: Option<String>,
        blocks_count: u64,
    ) -> Box<Stream<Item = BlockchainTransaction, Error = Error> + Send> {
        let self_clone = self.clone();
        Box::new(
            self.get_current_block_number()
                .into_stream()
                .map(move |current_block| {
                    let hash = start_block_hash.clone();
                    self_clone.last_stq_transactions_with_current_block(hash, blocks_count, current_block)
                }).flatten(),
        )
    }

    fn get_stq_transactions(&self, hash: String) -> Box<Stream<Item = BlockchainTransaction, Error = Error> + Send> {
        let self_clone = self.clone();
        Box::new(
            self.get_current_block_number()
                .into_stream()
                .map(move |current_block| {
                    let hash = hash.clone();
                    self_clone.get_stq_transactions_with_current_block(hash, current_block)
                }).flatten(),
        )
    }

    fn get_nonce(&self, address: EthereumAddress) -> Box<Future<Item = u64, Error = Error> + Send> {
        let address_clone2 = address.clone();
        let http_client = self.http_client.clone();
        let address_str = format!("0x{}", address);
        let request = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_getTransactionCount",
            "params": [address_str, "latest"]
        }).to_string();
        Box::new(
            Request::builder()
                .header("Content-Type", "application/json")
                .method("POST")
                .uri(self.infura_url.clone())
                .body(Body::from(request))
                .map_err(ectx!(ErrorSource::Hyper, ErrorKind::Internal => address_clone2))
                .into_future()
                .and_then(move |request| http_client.request(request))
                .and_then(|resp| read_body(resp.into_body()).map_err(ectx!(ErrorKind::Internal => address)))
                .and_then(|bytes| {
                    let bytes_clone = bytes.clone();
                    String::from_utf8(bytes).map_err(ectx!(ErrorContext::UTF8, ErrorKind::Internal => bytes_clone))
                }).and_then(|string| {
                    serde_json::from_str::<NonceResponse>(&string).map_err(ectx!(ErrorContext::Json, ErrorKind::Internal => string.clone()))
                }).and_then(|resp| {
                    u64::from_str_radix(&resp.result[2..], 16).map_err(ectx!(ErrorContext::Hex, ErrorKind::Internal => resp.result))
                }),
        )
    }

    fn send_raw_tx(&self, tx: RawEthereumTransaction) -> Box<Future<Item = TxHash, Error = Error> + Send> {
        let tx_clone2 = tx.clone();
        let http_client = self.http_client.clone();
        let tx_str = format!("0x{}", tx);
        let request = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_sendRawTransaction",
            "params": [tx_str]
        }).to_string();
        Box::new(
            Request::builder()
                .method("POST")
                .uri(self.infura_url.clone())
                .body(Body::from(request))
                .map_err(ectx!(ErrorSource::Hyper, ErrorKind::Internal => tx_clone2))
                .into_future()
                .and_then(move |request| http_client.request(request))
                .and_then(|resp| read_body(resp.into_body()).map_err(ectx!(ErrorKind::Internal => tx)))
                .and_then(|bytes| {
                    let bytes_clone = bytes.clone();
                    String::from_utf8(bytes).map_err(ectx!(ErrorContext::UTF8, ErrorKind::Internal => bytes_clone))
                }).and_then(|string| {
                    serde_json::from_str::<PostTransactionsResponse>(&string)
                        .map_err(ectx!(ErrorContext::Json, ErrorKind::Internal => string.clone()))
                }).map(|resp| resp.result),
        )
    }
}
