//! 主进程与剪贴板辅助进程之间的管道协议（ADR 0016 决策 5）。
//! 每条消息：u32 长度 + 1 字节标签 + 字段，整数一律小端。
// 编解码只有 macOS 的辅助进程两端在用，其他平台只用到类型。
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::io::{self, Read, Write};
use std::sync::Arc;

/// 单条消息上限，挡住读到错位长度时的超大分配。
const MAX_FRAME: usize = 1 << 30;

/// 辅助进程向主进程要的 Mac 类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Text = 1,
    Rtf = 2,
    Png = 3,
    Tiff = 4,
    FileUrl = 5,
}

impl Kind {
    fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            1 => Self::Text,
            2 => Self::Rtf,
            3 => Self::Png,
            4 => Self::Tiff,
            5 => Self::FileUrl,
            _ => return None,
        })
    }
}

/// 主进程应答的数据是哪种远端格式，辅助进程据此转换。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Format {
    UnicodeText = 1,
    Rtf = 2,
    Png = 3,
    /// CF_DIB 或 CF_DIBV5，按头部长度区分。
    Dib = 4,
    /// 下载好的本地路径，UTF-8。
    Path = 5,
}

impl Format {
    fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            1 => Self::UnicodeText,
            2 => Self::Rtf,
            3 => Self::Png,
            4 => Self::Dib,
            5 => Self::Path,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Data {
    pub(super) format: Format,
    pub(super) bytes: Arc<[u8]>,
}

/// 远端复制了什么；辅助进程据此写入延迟提供的条目。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Offer {
    pub(super) session: u64,
    pub(super) generation: u64,
    pub(super) text: bool,
    pub(super) rtf: bool,
    pub(super) image: bool,
    /// 顶层文件条目数，非 0 时只放文件。
    pub(super) files: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ToHelper {
    Offer(Offer),
    /// data 为 None 表示取不到。
    Reply {
        id: u64,
        data: Option<Data>,
    },
    /// 会话结束：剪贴板还是这个会话写入的就清空。
    Clear {
        session: u64,
    },
}

/// 剪贴板里的延迟数据被读取，辅助进程向主进程要数据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::rdp_session) struct Request {
    pub(super) id: u64,
    pub(super) session: u64,
    pub(super) generation: u64,
    pub(super) kind: Kind,
    /// 文件条目的顶层序号；其他类型为 0。
    pub(super) index: u32,
}

const TAG_OFFER: u8 = 1;
const TAG_REPLY: u8 = 2;
const TAG_CLEAR: u8 = 3;
const TAG_REQUEST: u8 = 1;

pub(super) fn write_to_helper(w: &mut impl Write, msg: &ToHelper) -> io::Result<()> {
    let mut head = Vec::with_capacity(32);
    let mut body: &[u8] = &[];
    match msg {
        ToHelper::Offer(offer) => {
            head.push(TAG_OFFER);
            head.extend_from_slice(&offer.session.to_le_bytes());
            head.extend_from_slice(&offer.generation.to_le_bytes());
            head.push(u8::from(offer.text) | u8::from(offer.rtf) << 1 | u8::from(offer.image) << 2);
            head.extend_from_slice(&offer.files.to_le_bytes());
        }
        ToHelper::Reply { id, data } => {
            head.push(TAG_REPLY);
            head.extend_from_slice(&id.to_le_bytes());
            match data {
                Some(data) => {
                    head.push(data.format as u8);
                    body = &data.bytes;
                }
                None => head.push(0),
            }
        }
        ToHelper::Clear { session } => {
            head.push(TAG_CLEAR);
            head.extend_from_slice(&session.to_le_bytes());
        }
    }
    write_frame(w, &head, body)
}

pub(super) fn read_to_helper(r: &mut impl Read) -> io::Result<ToHelper> {
    let frame = read_frame(r)?;
    let mut f = Fields(&frame);
    let msg = match f.u8()? {
        TAG_OFFER => {
            let session = f.u64()?;
            let generation = f.u64()?;
            let flags = f.u8()?;
            ToHelper::Offer(Offer {
                session,
                generation,
                text: flags & 1 != 0,
                rtf: flags & 2 != 0,
                image: flags & 4 != 0,
                files: f.u32()?,
            })
        }
        TAG_REPLY => {
            let id = f.u64()?;
            let data = match f.u8()? {
                0 => None,
                v => Some(Data {
                    format: Format::from_u8(v).ok_or_else(invalid)?,
                    bytes: f.rest().into(),
                }),
            };
            ToHelper::Reply { id, data }
        }
        TAG_CLEAR => ToHelper::Clear { session: f.u64()? },
        _ => return Err(invalid()),
    };
    Ok(msg)
}

pub(super) fn write_request(w: &mut impl Write, req: &Request) -> io::Result<()> {
    let mut head = Vec::with_capacity(30);
    head.push(TAG_REQUEST);
    head.extend_from_slice(&req.id.to_le_bytes());
    head.extend_from_slice(&req.session.to_le_bytes());
    head.extend_from_slice(&req.generation.to_le_bytes());
    head.push(req.kind as u8);
    head.extend_from_slice(&req.index.to_le_bytes());
    write_frame(w, &head, &[])
}

pub(super) fn read_request(r: &mut impl Read) -> io::Result<Request> {
    let frame = read_frame(r)?;
    let mut f = Fields(&frame);
    if f.u8()? != TAG_REQUEST {
        return Err(invalid());
    }
    Ok(Request {
        id: f.u64()?,
        session: f.u64()?,
        generation: f.u64()?,
        kind: Kind::from_u8(f.u8()?).ok_or_else(invalid)?,
        index: f.u32()?,
    })
}

fn write_frame(w: &mut impl Write, head: &[u8], body: &[u8]) -> io::Result<()> {
    let len = u32::try_from(head.len() + body.len()).map_err(|_| invalid())?;
    w.write_all(&len.to_le_bytes())?;
    w.write_all(head)?;
    w.write_all(body)?;
    w.flush()
}

fn read_frame(r: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut len = [0; 4];
    r.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_FRAME {
        return Err(invalid());
    }
    let mut frame = vec![0; len];
    r.read_exact(&mut frame)?;
    Ok(frame)
}

fn invalid() -> io::Error {
    io::Error::from(io::ErrorKind::InvalidData)
}

struct Fields<'a>(&'a [u8]);

impl<'a> Fields<'a> {
    fn take<const N: usize>(&mut self) -> io::Result<[u8; N]> {
        let (head, rest) = self.0.split_first_chunk::<N>().ok_or_else(invalid)?;
        self.0 = rest;
        Ok(*head)
    }

    fn u8(&mut self) -> io::Result<u8> {
        Ok(self.take::<1>()?[0])
    }

    fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(self.take()?))
    }

    fn u64(&mut self) -> io::Result<u64> {
        Ok(u64::from_le_bytes(self.take()?))
    }

    fn rest(&mut self) -> &'a [u8] {
        std::mem::take(&mut self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(msg: ToHelper) {
        let mut buf = Vec::new();
        write_to_helper(&mut buf, &msg).unwrap();
        assert_eq!(read_to_helper(&mut buf.as_slice()).unwrap(), msg);
    }

    #[test]
    fn to_helper_messages_roundtrip() {
        roundtrip(ToHelper::Offer(Offer {
            session: 7,
            generation: u64::MAX,
            text: true,
            rtf: false,
            image: true,
            files: 0,
        }));
        roundtrip(ToHelper::Offer(Offer {
            files: 3,
            ..Offer::default()
        }));
        roundtrip(ToHelper::Reply {
            id: 42,
            data: Some(Data {
                format: Format::Dib,
                bytes: vec![1, 2, 3].into(),
            }),
        });
        roundtrip(ToHelper::Reply { id: 1, data: None });
        roundtrip(ToHelper::Clear { session: 9 });
    }

    #[test]
    fn requests_roundtrip_back_to_back() {
        let a = Request {
            id: 1,
            session: 2,
            generation: 3,
            kind: Kind::FileUrl,
            index: 4,
        };
        let b = Request {
            kind: Kind::Tiff,
            index: 0,
            ..a
        };
        let mut buf = Vec::new();
        write_request(&mut buf, &a).unwrap();
        write_request(&mut buf, &b).unwrap();
        let mut r = buf.as_slice();
        assert_eq!(read_request(&mut r).unwrap(), a);
        assert_eq!(read_request(&mut r).unwrap(), b);
        assert!(read_request(&mut r).is_err());
    }

    #[test]
    fn truncated_or_unknown_frames_fail() {
        let mut buf = Vec::new();
        write_to_helper(&mut buf, &ToHelper::Clear { session: 1 }).unwrap();
        assert!(read_to_helper(&mut &buf[..buf.len() - 1]).is_err());
        assert!(read_to_helper(&mut [1u8, 0, 0, 0, 9].as_slice()).is_err());
    }
}
