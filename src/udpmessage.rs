use std::{mem, ptr, fmt};
use std::net::{SocketAddr, SocketAddrV4, Ipv4Addr};
use std::u16;

pub use super::{Mac, Error, Token, EthernetFrame, eth_decode, eth_encode};

unsafe fn as_obj<T>(data: &[u8]) -> &T {
    assert!(data.len() >= mem::size_of::<T>());
    mem::transmute(data.as_ptr())
}

#[derive(PartialEq)]
pub enum Message<'a> {
    Frame(EthernetFrame<'a>),
    Peers(Vec<SocketAddr>),
    GetPeers,
    Close,
}

impl<'a> fmt::Debug for Message<'a> {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        match self {
            &Message::Frame(ref frame) => write!(formatter, "Frame({:?})", frame),
            &Message::Peers(ref peers) => {
                try!(write!(formatter, "Peers ["));
                let mut first = true;
                for p in peers {
                    if !first {
                        try!(write!(formatter, ", "));
                    }
                    first = false;
                    try!(p.fmt(formatter));
                }
                write!(formatter, "]")
            },
            &Message::GetPeers => write!(formatter, "GetPeers"),
            &Message::Close => write!(formatter, "Close"),
        }
    }
}

pub fn decode(data: &[u8]) -> Result<(Token, Message), Error> {
    if data.len() < 1 + mem::size_of::<Token>() {
        return Err(Error::ParseError("Empty message"));
    }
    let mut pos = 0;
    let token = Token::from_be(* unsafe { as_obj::<Token>(&data[pos..]) });
    pos += mem::size_of::<Token>();
    match data[pos] {
        0 => {
            pos += 1;
            Ok((token, Message::Frame(try!(eth_decode(&data[pos..])))))
        },
        1 => {
            pos += 1;
            if data.len() < pos + 1 {
                return Err(Error::ParseError("Empty peers"));
            }
            let count = data[pos];
            pos += 1;
            let len = count as usize * 6;
            if data.len() < pos + len {
                return Err(Error::ParseError("Peer data too short"));
            }
            let mut peers = Vec::with_capacity(count as usize);
            for _ in 0..count {
                let (ip, port) = unsafe {
                    let ip = as_obj::<[u8; 4]>(&data[pos..]);
                    pos += 4;
                    let port = *as_obj::<u16>(&data[pos..]);
                    let port = u16::from_be(port);
                    pos += 2;
                    (ip, port)
                };
                let addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(ip[0], ip[1], ip[2], ip[3]), port));
                peers.push(addr);
            }
            Ok((token, Message::Peers(peers)))
        },
        2 => Ok((token, Message::GetPeers)),
        3 => Ok((token, Message::Close)),
        _ => Err(Error::ParseError("Unknown message type"))
    }
}

pub fn encode(token: Token, msg: &Message, buf: &mut [u8]) -> usize {
    assert!(buf.len() >= mem::size_of::<Token>() + 1);
    let mut pos = 0;
    let token_dat = unsafe { mem::transmute::<Token, [u8; 8]>(token.to_be()) };
    unsafe { ptr::copy_nonoverlapping(token_dat.as_ptr(), buf[pos..].as_mut_ptr(), token_dat.len()) };
    pos += token_dat.len();
    match msg {
        &Message::Frame(ref frame) => {
            buf[pos] = 0;
            pos += 1;
            pos += eth_encode(&frame, &mut buf[pos..])
        },
        &Message::Peers(ref peers) => {
            buf[pos] = 1;
            pos += 1;
            let count_pos = pos;
            pos += 1;
            assert!(buf.len() >= 2 + peers.len() * mem::size_of::<SocketAddrV4>());
            let mut count = 0;
            for p in peers {
                match p {
                    &SocketAddr::V4(addr) => {
                        let ip = addr.ip().octets();
                        let port = addr.port();
                        unsafe {
                            ptr::copy_nonoverlapping(ip.as_ptr(), buf[pos..].as_mut_ptr(), ip.len());
                            pos += ip.len();
                            let port = mem::transmute::<u16, [u8; 2]>(port.to_be());
                            ptr::copy_nonoverlapping(port.as_ptr(), buf[pos..].as_mut_ptr(), port.len());
                            pos += port.len();
                        }
                        count += 1;
                    },
                    &SocketAddr::V6(_addr) => unimplemented!()
                }
            };
            buf[count_pos] = count;
        },
        &Message::GetPeers => {
            buf[pos] = 2;
            pos += 1;
        },
        &Message::Close => {
            buf[pos] = 3;
            pos += 1;
        }
    }
    pos
}


#[test]
fn encode_message_packet() {
    let token = 134;
    let src = Mac([1,2,3,4,5,6]);
    let dst = Mac([7,8,9,10,11,12]);
    let payload = [1,2,3,4,5];
    let msg = Message::Frame(EthernetFrame{src: &src, dst: &dst, vlan: 0, payload: &payload});
    let mut buf = [0; 1024];
    let size = encode(token, &msg, &mut buf[..]);
    assert_eq!(size, 26);
    assert_eq!(&buf[..9], &[0,0,0,0,0,0,0,134,0]);
    let (token2, msg2) = decode(&buf[..size]).unwrap();
    assert_eq!(token, token2);
    assert_eq!(msg, msg2);
}

#[test]
fn encode_message_peers() {
    use std::str::FromStr;
    let token = 134;
    let msg = Message::Peers(vec![SocketAddr::from_str("1.2.3.4:123").unwrap(), SocketAddr::from_str("5.6.7.8:12345").unwrap()]);
    let mut buf = [0; 1024];
    let size = encode(token, &msg, &mut buf[..]);
    assert_eq!(size, 22);
    assert_eq!(&buf[..size], &[0,0,0,0,0,0,0,134,1,2,1,2,3,4,0,123,5,6,7,8,48,57]);
    let (token2, msg2) = decode(&buf[..size]).unwrap();
    assert_eq!(token, token2);
    assert_eq!(msg, msg2);
}

#[test]
fn encode_message_getpeers() {
    let token = 134;
    let msg = Message::GetPeers;
    let mut buf = [0; 1024];
    let size = encode(token, &msg, &mut buf[..]);
    assert_eq!(size, 9);
    assert_eq!(&buf[..size], &[0,0,0,0,0,0,0,134,2]);
    let (token2, msg2) = decode(&buf[..size]).unwrap();
    assert_eq!(token, token2);
    assert_eq!(msg, msg2);
}

#[test]
fn encode_message_close() {
    let token = 134;
    let msg = Message::Close;
    let mut buf = [0; 1024];
    let size = encode(token, &msg, &mut buf[..]);
    assert_eq!(size, 9);
    assert_eq!(&buf[..size], &[0,0,0,0,0,0,0,134,3]);
    let (token2, msg2) = decode(&buf[..size]).unwrap();
    assert_eq!(token, token2);
    assert_eq!(msg, msg2);
}