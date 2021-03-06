use models::*;

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PostBitcoinTransactionRequest {
    pub raw: RawBitcoinTransaction,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PostEthereumTransactionRequest {
    pub raw: RawEthereumTransaction,
}
