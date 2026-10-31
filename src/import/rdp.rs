//! `.rdp` files, as Windows' Remote Desktop Connection, Remmina and most
//! other clients write them: one `name:type:value` setting per line.
//! Settings Mote doesn't have yet are skipped on import and absent on export.

use crate::connection::{Connection, Network, split_address};

/// The connection in an `.rdp` file, named `name` unless the file says.
pub(crate) fn parse(bytes: &[u8], name: &str) -> Option<Connection> {
    let text = decode(bytes);
    let mut conn = Connection {
        name: name.to_owned(),
        ..Connection::default()
    };
    let mut port = None;
    for line in text.lines() {
        let mut parts = line.splitn(3, ':');
        let (Some(key), Some(_kind), Some(value)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let value = value.trim();
        match key.trim().to_ascii_lowercase().as_str() {
            "full address" => {
                let (host, p) = split_address(value)?;
                conn.host = host;
                port = port.or(p);
            }
            "server port" => port = value.parse().ok().filter(|&p| p != 0),
            "username" => conn.username = value.to_owned(),
            "domain" => conn.domain = value.to_owned(),
            "connection type" => {
                conn.network = value
                    .parse()
                    .map_or_else(|_| Network::default(), Network::from_number);
            }
            _ => {}
        }
    }
    if conn.host.is_empty() {
        return None;
    }
    if let Some(port) = port {
        conn.port = port;
    }
    Some(conn.normalized())
}

/// The connection as an `.rdp` file: UTF-16 with a byte-order mark, which
/// is what Windows writes and what every client reads.
pub(crate) fn export(conn: &Connection) -> Vec<u8> {
    let mut lines = vec![format!("full address:s:{}", conn.address())];
    if let Some(account) = conn.account() {
        lines.push(format!("username:s:{account}"));
    }
    lines.push("prompt for credentials:i:0".to_owned());
    lines.push(format!("connection type:i:{}", conn.network.number()));
    let text = lines.join("\r\n") + "\r\n";
    let mut out = vec![0xFF, 0xFE];
    for unit in text.encode_utf16() {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    out
}

fn decode(bytes: &[u8]) -> String {
    let utf16 = |be: bool| {
        let units: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| {
                if be {
                    u16::from_be_bytes(c)
                } else {
                    u16::from_le_bytes(c)
                }
            })
            .collect();
        String::from_utf16_lossy(&units)
    };
    match bytes {
        [0xFF, 0xFE, ..] => utf16(false),
        [0xFE, 0xFF, ..] => utf16(true),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_what_windows_writes() {
        let text = "screen mode id:i:2\r\nfull address:s:box.lan:3390\r\nusername:s:CORP\\jo\r\n";
        let mut bytes = vec![0xFF, 0xFE];
        for u in text.encode_utf16() {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        let c = parse(&bytes, "Office").unwrap();
        assert_eq!(
            (c.name.as_str(), c.host.as_str(), c.port),
            ("Office", "box.lan", 3390)
        );
        assert_eq!((c.domain.as_str(), c.username.as_str()), ("CORP", "jo"));
    }

    #[test]
    fn export_reads_back() {
        let c = Connection {
            host: "box".into(),
            port: 3390,
            username: "jo".into(),
            network: Network::Satellite,
            ..Connection::default()
        };
        let back = parse(&export(&c), "box").unwrap();
        assert!(back.same_target(&c));
        assert_eq!(back.network, Network::Satellite);
    }

    #[test]
    fn needs_an_address() {
        assert!(parse(b"username:s:jo\n", "x").is_none());
    }
}
