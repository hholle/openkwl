//! Minimal mDNS responder logic (RFC 6762), new code: answers "who is `<hostname>.local`?" with
//! the unit's IPv4 address, so browsers find it as `http://openkwl.local/`. Nothing else: no
//! service discovery, no probing for name conflicts (the name is the one from the settings).
//!
//! The firmware owns the UDP socket on port 5353 and the multicast group 224.0.0.251; this
//! module only reads queries and writes answers, so it is tested on the host.

/// mDNS port and IPv4 group.
pub const PORT: u16 = 5353;
pub const GROUP: [u8; 4] = [224, 0, 0, 251];
/// Answer lifetime (RFC 6762 recommends 120 s for address records).
pub const TTL_S: u32 = 120;

const TYPE_A: u16 = 1;
const TYPE_ANY: u16 = 255;
const CLASS_IN: u16 = 1;
const CACHE_FLUSH: u16 = 0x8000;
/// Longest DNS name (RFC 1035).
const NAME_MAX: usize = 255;

/// How to answer a query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reply {
    /// Send `len` bytes to the multicast group (normal mDNS query from port 5353).
    Multicast(usize),
    /// Send `len` bytes back to the sender only (legacy one-shot query from another port, or
    /// the question asked for a unicast reply).
    Unicast(usize),
}

fn u16_at(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*b.get(i)?, *b.get(i + 1)?]))
}

/// Compares the (possibly compressed) name at `pos` with `host` + `.local`, ignoring case.
/// Returns the offset after the name in the question.
fn match_name(msg: &[u8], mut pos: usize, host: &str) -> Option<(bool, usize)> {
    let mut expect = host.split('.').chain(core::iter::once("local"));
    let mut end = None;
    let mut matches = true;
    let mut hops = 0;
    let mut total = 0;
    loop {
        let len = *msg.get(pos)? as usize;
        if len & 0xC0 == 0xC0 {
            // Compression pointer.
            let target = (u16_at(msg, pos)? & 0x3FFF) as usize;
            end.get_or_insert(pos + 2);
            hops += 1;
            if hops > 8 || target >= msg.len() {
                return None;
            }
            pos = target;
            continue;
        }
        if len & 0xC0 != 0 {
            return None;
        }
        if len == 0 {
            let after = end.unwrap_or(pos + 1);
            return Some((matches && expect.next().is_none(), after));
        }
        let label = msg.get(pos + 1..pos + 1 + len)?;
        total += len + 1;
        if total > NAME_MAX {
            return None;
        }
        match expect.next() {
            Some(e) if e.as_bytes().eq_ignore_ascii_case(label) => {}
            _ => matches = false,
        }
        pos += 1 + len;
    }
}

fn put(out: &mut [u8], at: &mut usize, bytes: &[u8]) -> Option<()> {
    out.get_mut(*at..*at + bytes.len())?.copy_from_slice(bytes);
    *at += bytes.len();
    Some(())
}

fn put_name(out: &mut [u8], at: &mut usize, host: &str) -> Option<()> {
    for label in host.split('.').chain(core::iter::once("local")) {
        if label.is_empty() || label.len() > 63 {
            return None;
        }
        put(out, at, &[label.len() as u8])?;
        put(out, at, label.as_bytes())?;
    }
    put(out, at, &[0])
}

fn put_a_record(out: &mut [u8], at: &mut usize, host: &str, ip: [u8; 4], class: u16) -> Option<()> {
    put_name(out, at, host)?;
    put(out, at, &TYPE_A.to_be_bytes())?;
    put(out, at, &class.to_be_bytes())?;
    put(out, at, &TTL_S.to_be_bytes())?;
    put(out, at, &4u16.to_be_bytes())?;
    put(out, at, &ip)
}

/// Unsolicited announcement of `<host>.local` → `ip`, sent after the address changes.
pub fn announcement(host: &str, ip: [u8; 4], out: &mut [u8]) -> Option<usize> {
    let mut at = 0;
    // id 0, flags: response + authoritative, 0 questions, 1 answer.
    put(out, &mut at, &[0, 0, 0x84, 0x00, 0, 0, 0, 1, 0, 0, 0, 0])?;
    put_a_record(out, &mut at, host, ip, CLASS_IN | CACHE_FLUSH)?;
    Some(at)
}

/// Answers `query` (received from `src_port`) if it asks for the A record of `<host>.local`;
/// `None` for everything else (responses, other names, other types, malformed packets).
pub fn answer(query: &[u8], src_port: u16, host: &str, ip: [u8; 4], out: &mut [u8]) -> Option<Reply> {
    let flags = u16_at(query, 2)?;
    if flags & 0x8000 != 0 || flags & 0x7800 != 0 {
        return None; // a response, or not a standard query
    }
    let questions = u16_at(query, 4)?;
    let mut pos = 12;
    for _ in 0..questions {
        let (ours, after) = match_name(query, pos, host)?;
        let qtype = u16_at(query, after)?;
        let qclass = u16_at(query, after + 2)?;
        let question = &query[pos..after + 4];
        pos = after + 4;
        if !ours || !(qtype == TYPE_A || qtype == TYPE_ANY) || qclass & 0x7FFF != CLASS_IN {
            continue;
        }
        let legacy = src_port != PORT;
        let unicast = legacy || qclass & 0x8000 != 0;
        let mut at = 0;
        if legacy {
            // RFC 6762 §6.7: echo id and question, no cache-flush bit.
            put(out, &mut at, &query[0..2])?;
            put(out, &mut at, &[0x84, 0x00, 0, 1, 0, 1, 0, 0, 0, 0])?;
            put(out, &mut at, question)?;
            put_a_record(out, &mut at, host, ip, CLASS_IN)?;
        } else {
            put(out, &mut at, &[0, 0, 0x84, 0x00, 0, 0, 0, 1, 0, 0, 0, 0])?;
            put_a_record(out, &mut at, host, ip, CLASS_IN | CACHE_FLUSH)?;
        }
        return Some(if unicast { Reply::Unicast(at) } else { Reply::Multicast(at) });
    }
    None
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    /// A query for `name` (labels) with `qtype` and `qclass`.
    fn query(id: u16, labels: &[&str], qtype: u16, qclass: u16) -> Vec<u8> {
        let mut q = Vec::new();
        q.extend_from_slice(&id.to_be_bytes());
        q.extend_from_slice(&[0, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
        for l in labels {
            q.push(l.len() as u8);
            q.extend_from_slice(l.as_bytes());
        }
        q.push(0);
        q.extend_from_slice(&qtype.to_be_bytes());
        q.extend_from_slice(&qclass.to_be_bytes());
        q
    }

    const IP: [u8; 4] = [192, 168, 1, 23];

    #[test]
    fn answers_its_own_name_case_insensitively() {
        let mut out = [0u8; 128];
        let q = query(0, &["Helios", "LOCAL"], TYPE_A, CLASS_IN);
        let Some(Reply::Multicast(n)) = answer(&q, PORT, "helios", IP, &mut out) else { panic!() };
        let r = &out[..n];
        assert_eq!(&r[2..4], &[0x84, 0x00], "response, authoritative");
        assert_eq!(&r[4..8], &[0, 0, 0, 1], "no question, one answer");
        assert_eq!(&r[n - 4..], &IP);
        assert_eq!(&r[12..26], b"\x06helios\x05local\x00");
        assert_eq!(u16_at(r, 28), Some(CLASS_IN | CACHE_FLUSH));
    }

    #[test]
    fn ignores_other_names_types_and_responses() {
        let mut out = [0u8; 128];
        assert_eq!(answer(&query(0, &["printer", "local"], TYPE_A, CLASS_IN), PORT, "helios", IP, &mut out), None);
        assert_eq!(answer(&query(0, &["helios", "lan"], TYPE_A, CLASS_IN), PORT, "helios", IP, &mut out), None);
        assert_eq!(answer(&query(0, &["helios", "local"], 28, CLASS_IN), PORT, "helios", IP, &mut out), None, "AAAA");
        let mut resp = query(0, &["helios", "local"], TYPE_A, CLASS_IN);
        resp[2] = 0x84;
        assert_eq!(answer(&resp, PORT, "helios", IP, &mut out), None);
        assert_eq!(answer(&[0, 1, 2], PORT, "helios", IP, &mut out), None, "truncated");
    }

    #[test]
    fn legacy_and_unicast_queries_get_a_unicast_reply() {
        let mut out = [0u8; 128];
        let q = query(0x1234, &["helios", "local"], TYPE_ANY, CLASS_IN);
        let Some(Reply::Unicast(n)) = answer(&q, 51000, "helios", IP, &mut out) else { panic!() };
        assert_eq!(&out[0..2], &[0x12, 0x34], "id echoed");
        assert_eq!(&out[4..8], &[0, 1, 0, 1], "question echoed");
        assert_eq!(&out[n - 4..n], &IP);
        let q = query(0, &["helios", "local"], TYPE_A, CLASS_IN | 0x8000);
        assert!(matches!(answer(&q, PORT, "helios", IP, &mut out), Some(Reply::Unicast(_))));
    }

    #[test]
    fn follows_compression_pointers() {
        // Two questions; the second one ("openkwl.local") points into the first ("x.local").
        let mut q: Vec<u8> = Vec::new();
        q.extend_from_slice(&[0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0]);
        q.extend_from_slice(b"\x01x\x05local\x00\x00\x01\x00\x01"); // name at 12, "local" at 14
        q.extend_from_slice(b"\x06helios\xc0\x0e\x00\x01\x00\x01");
        let mut out = [0u8; 128];
        assert!(matches!(answer(&q, PORT, "helios", IP, &mut out), Some(Reply::Multicast(_))));
        // A pointer loop is rejected, not followed forever.
        let looped = [0u8, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0xc0, 12, 0, 1, 0, 1];
        assert_eq!(answer(&looped, PORT, "helios", IP, &mut out), None);
    }

    #[test]
    fn announcement_matches_the_answer() {
        let (mut a, mut b) = ([0u8; 64], [0u8; 64]);
        let n = announcement("helios", IP, &mut a).unwrap();
        let q = query(0, &["helios", "local"], TYPE_A, CLASS_IN);
        assert_eq!(answer(&q, PORT, "helios", IP, &mut b), Some(Reply::Multicast(n)));
        assert_eq!(a[..n], b[..n]);
        assert_eq!(announcement("", IP, &mut a), None, "empty name");
    }
}
