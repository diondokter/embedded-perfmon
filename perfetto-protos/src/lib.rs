#[path = "perfetto.protos.rs"]
pub mod idl;

pub use prost;
use prost::Message;

pub fn serialize_trace(trace: idl::Trace) -> Vec<u8> {
    trace.encode_to_vec()
}
