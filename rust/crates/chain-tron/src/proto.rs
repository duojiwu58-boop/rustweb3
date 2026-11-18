// Minimal wire schema from tronprotocol/protocol at 2a678934da3992b1a67f975769bbb2d31989451f.
// Unknown fields are rejected by exact decode/re-encode at EVERY nested layer.
#[derive(Clone, PartialEq, prost::Message)]
pub struct Any {
    #[prost(string, tag = "1")]
    pub type_url: String,
    #[prost(bytes = "vec", tag = "2")]
    pub value: Vec<u8>,
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct Contract {
    #[prost(int32, tag = "1")]
    pub kind: i32,
    #[prost(message, optional, tag = "2")]
    pub parameter: Option<Any>,
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct Raw {
    #[prost(bytes = "vec", tag = "1")]
    pub ref_block_bytes: Vec<u8>,
    #[prost(bytes = "vec", tag = "4")]
    pub ref_block_hash: Vec<u8>,
    #[prost(int64, tag = "8")]
    pub expiration: i64,
    #[prost(message, repeated, tag = "11")]
    pub contracts: Vec<Contract>,
    #[prost(int64, tag = "14")]
    pub timestamp: i64,
    #[prost(int64, tag = "18")]
    pub fee_limit: i64,
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct Native {
    #[prost(bytes = "vec", tag = "1")]
    pub owner: Vec<u8>,
    #[prost(bytes = "vec", tag = "2")]
    pub recipient: Vec<u8>,
    #[prost(int64, tag = "3")]
    pub amount: i64,
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct Trigger {
    #[prost(bytes = "vec", tag = "1")]
    pub owner: Vec<u8>,
    #[prost(bytes = "vec", tag = "2")]
    pub contract: Vec<u8>,
    #[prost(bytes = "vec", tag = "4")]
    pub data: Vec<u8>,
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct Signed {
    #[prost(message, optional, tag = "1")]
    pub raw: Option<Raw>,
    #[prost(bytes = "vec", repeated, tag = "2")]
    pub signatures: Vec<Vec<u8>>,
}
