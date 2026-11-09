//! The game server's methods, as the emulated terminal answers them.
//!
//! A cabinet sends `POST /method/<name>` with a `wm.protobuf.<Name>Request`
//! body (`Content-Type: application/x-protobuf; revision=...`) and expects
//! the matching `<Name>Response`. The schemas are the game's own (embedded
//! in the executable); proto2 `required` fields must all be present or the
//! game rejects the response.
//!
//! No `std`, so the responses are unit-tested on the host.
use crate::terminal::Writer;

/// `wm.protobuf.ErrorCode`.
const SUCCESS: i64 = 0;

/// The cabinet's shop as the server knows it.
const PLACE_ID: &[u8] = b"JPN0000";
const SHOP_NAME: &[u8] = b"Banana Pepper";
const REGION_ID: i64 = 1;
const COUNTRY: &[u8] = b"JPN";

/// A request: a method (`POST /method/<name>`) or a resource (`GET
/// /resource/<name>?<query>`).
#[derive(Debug, PartialEq, Eq)]
pub enum Request<'a> {
    Method(&'a [u8]),
    Resource(&'a [u8]),
}

/// The request a path names, if any.
pub fn request(path: &[u8]) -> Option<Request<'_>> {
    fn name(rest: &[u8]) -> &[u8] {
        let end = rest.iter().position(|&b| b == b'?' || b == b'/').unwrap_or(rest.len());
        &rest[..end]
    }
    if let Some(rest) = path.strip_prefix(b"/method/") {
        Some(Request::Method(name(rest)))
    } else {
        path.strip_prefix(b"/resource/").map(|rest| Request::Resource(name(rest)))
    }
}

/// The response body for a request, written to `out`; `None` when this
/// server has no answer.
pub fn answer(request: &Request, body: &[u8], out: &mut [u8]) -> Option<usize> {
    match request {
        Request::Method(name) => respond(name, body, out),
        Request::Resource(name) => resource(name, out),
    }
}

/// Resources are lists (repeated fields only); every list is empty but the
/// shop list, which holds the cabinet's own shop.
fn resource(name: &[u8], out: &mut [u8]) -> Option<usize> {
    let mut w = Writer::new(out);
    match name {
        b"place_list" => {
            let mut place = [0u8; 64];
            let mut p = Writer::new(&mut place);
            p.bytes_field(1, PLACE_ID).bytes_field(2, SHOP_NAME).int(3, REGION_ID).bytes_field(4, COUNTRY);
            w.bytes_field(1, p.bytes());
        }
        b"ranking" | b"crown_list" | b"file_list" | b"ghost_list" | b"lock_wanted_list" => {}
        _ => return None,
    }
    Some(w.len())
}

/// The response body for `method`, written to `out`. Responses whose only
/// required field is the error code are answered for any method; the rest
/// have their required fields filled here.
fn respond(method: &[u8], _request: &[u8], out: &mut [u8]) -> Option<usize> {
    let mut w = Writer::new(out);
    match method {
        b"register_system_info" => {
            // RegisterSystemInfoResponse.
            let mut version = [0u8; 32];
            let mut v = Writer::new(&mut version);
            v.int(1, 0).int(2, 0).int(3, 0).int(4, 0).int(5, 0);
            w.int(1, SUCCESS)
                .int(2, REGION_ID)
                .bytes_field(3, PLACE_ID)
                .bytes_field(10, v.bytes()) // feature_version
                .int(28, 0) // ghost_selection_min_redo_wait
                .int(29, 0); // ghost_selection_max_redo_wait
        }
        b"ping" => {
            w.int(1, SUCCESS).int(2, 1);
        }
        _ => {
            w.int(1, SUCCESS);
        }
    }
    Some(w.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_come_from_the_path() {
        assert_eq!(request(b"/method/ping"), Some(Request::Method(b"ping")));
        assert_eq!(request(b"/method/load_user?x=1"), Some(Request::Method(b"load_user")));
        assert_eq!(request(b"/resource/place_list?"), Some(Request::Resource(b"place_list")));
        assert_eq!(request(b"/"), None);
    }

    #[test]
    fn responses_start_with_the_error_code() {
        let mut out = [0u8; 128];
        let n = answer(&Request::Method(b"ping"), &[], &mut out).unwrap();
        assert_eq!(&out[..n], &[0x08, 0x00, 0x10, 0x01]);
        let n = answer(&Request::Method(b"save_charge"), &[], &mut out).unwrap();
        assert_eq!(&out[..n], &[0x08, 0x00]);
        assert_eq!(answer(&Request::Resource(b"ranking"), &[], &mut out), Some(0));
        assert!(answer(&Request::Resource(b"no_such_resource"), &[], &mut out).is_none());
    }
}
