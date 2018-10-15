mod error;
mod responses;

use std::sync::Arc;

use hyper::{Body, Request};

use self::error::*;
use self::responses::UtxosResponse;
use super::HttpClient;
use config::Mode;
use models::*;
use prelude::*;
use serde_json;
use utils::read_body;

pub trait BitcoinClient: Send + Sync + 'static {
    fn get_utxos(&self, address: BitcoinAddress) -> Box<Future<Item = Vec<Utxo>, Error = Error> + Send>;
    fn send_raw_tx(&self, tx: BitcoinTransaction) -> Box<Future<Item = TxHash, Error = Error> + Send>;
}

#[derive(Clone)]
pub struct BitcoinClientImpl {
    http_client: Arc<HttpClient>,
    mode: Mode,
    blockcypher_token: String,
}

impl BitcoinClientImpl {
    pub fn new(http_client: Arc<HttpClient>, blockcypher_token: String, mode: Mode) -> Self {
        Self {
            http_client,
            blockcypher_token,
            mode,
        }
    }
}

impl BitcoinClient for BitcoinClientImpl {
    fn get_utxos(&self, address: BitcoinAddress) -> Box<Future<Item = Vec<Utxo>, Error = Error> + Send> {
        let address_clone = address.clone();
        let address_clone2 = address.clone();
        let http_client = self.http_client.clone();
        let uri_base = match self.mode {
            Mode::Production => "https://blockchain.info",
            _ => "https://testnet.blockchain.info",
        };
        Box::new(
            Request::builder()
                .method("GET")
                .uri(format!("{}/unspent?active={}", uri_base, address))
                .body(Body::empty())
                .map_err(ectx!(ErrorSource::Hyper, ErrorKind::Internal => address_clone2))
                .into_future()
                .and_then(move |request| http_client.request(request).map_err(ectx!(ErrorKind::Internal => address_clone)))
                .and_then(|resp| read_body(resp.into_body()).map_err(ectx!(ErrorKind::Internal => address)))
                .and_then(|bytes| {
                    let bytes_clone = bytes.clone();
                    String::from_utf8(bytes).map_err(ectx!(ErrorContext::UTF8, ErrorKind::Internal => bytes_clone))
                }).and_then(|string| {
                    serde_json::from_str::<UtxosResponse>(&string).map_err(ectx!(ErrorContext::Json, ErrorKind::Internal => string.clone()))
                }).map(|resp| resp.unspent_outputs.into_iter().map(From::from).collect()),
        )
    }

    fn send_raw_tx(&self, tx: BitcoinTransaction) -> Box<Future<Item = TxHash, Error = Error> + Send> {}
}
