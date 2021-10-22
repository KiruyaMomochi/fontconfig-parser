mod types;
mod util;

use std::io::BufRead;
use util::AttributeExt;

pub type Result<T> = std::result::Result<T, Error>;

pub use crate::types::*;

pub use quick_xml;

use quick_xml::{events::Event, Reader};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("XML error: {0}")]
    Xml(#[from] quick_xml::Error),
    #[error("DOCTYPE is not fontconfig")]
    UnmatchedDocType,
    #[error("Can't find fontconfig element")]
    NoFontconfig,
    #[error("Config format is invalid")]
    InvalidFormat,
    #[error("Unknown variant: {0}")]
    ParseError(#[from] strum::ParseError),
    #[error("Parse int error: {0}")]
    ParseIntError(#[from] std::num::ParseIntError),
    #[error("Parse float error: {0}")]
    ParseFloatError(#[from] std::num::ParseFloatError),
    #[error("Parse bool error: {0}")]
    ParseBoolError(#[from] std::str::ParseBoolError),
    #[error("Can't make Property {0:?} from value {1:?}")]
    PropertyConvertError(PropertyKind, Value),
    #[error("Can't make Property {0:?} from constant {1:?}")]
    ConstantPropertyError(PropertyKind, Constant),
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Xml(e.into())
    }
}

/// https://www.freedesktop.org/software/fontconfig/fontconfig-user.html
#[derive(Clone, Debug, Default)]
pub struct Document {
    pub description: String,
    pub dirs: Vec<Dir>,
    pub cache_dirs: Vec<CacheDir>,
    pub includes: Vec<Include>,
    pub matches: Vec<Match>,
    pub config: Config,
}

pub struct DocumentReader {
    buf: Vec<u8>,
}

impl DocumentReader {
    pub fn new() -> Self {
        Self {
            buf: Vec::with_capacity(128),
        }
    }

    /// Clear internal buffer
    pub fn clear(&mut self) {
        self.buf.clear();
    }

    fn read_string<B: BufRead>(&mut self, tag: &[u8], reader: &mut Reader<B>) -> Result<String> {
        loop {
            match reader.read_event(&mut self.buf)? {
                Event::Start(s) => {
                    if s.name() == tag {
                        break Ok(reader.read_text(tag, &mut self.buf)?);
                    } else {
                        break Err(Error::InvalidFormat);
                    }
                }
                Event::Eof => {
                    break Err(quick_xml::Error::UnexpectedEof(format!("Expect {:?}", tag)).into())
                }
                _ => {}
            }
        }
    }

    fn read_value<B: BufRead>(&mut self, reader: &mut Reader<B>) -> Result<Value> {
        loop {
            match reader.read_event(&mut self.buf)? {
                Event::Start(s) => match s.name() {
                    b"string" => {
                        break Ok(Value::String(reader.read_text(b"string", &mut self.buf)?));
                    }
                    b"double" => {
                        break Ok(Value::Double(
                            reader.read_text(b"double", &mut self.buf)?.parse()?,
                        ));
                    }
                    b"int" => {
                        break Ok(Value::Int(
                            reader.read_text(b"int", &mut self.buf)?.parse()?,
                        ));
                    }
                    b"bool" => {
                        break Ok(Value::Bool(
                            reader.read_text(b"bool", &mut self.buf)?.parse()?,
                        ));
                    }
                    b"const" => {
                        break Ok(Value::Const(
                            reader.read_text(b"const", &mut self.buf)?.parse()?,
                        ));
                    }
                    b"matrix" => {
                        break Ok(Value::Matrix([
                            self.read_string(b"double", reader)?.parse()?,
                            self.read_string(b"double", reader)?.parse()?,
                            self.read_string(b"double", reader)?.parse()?,
                            self.read_string(b"double", reader)?.parse()?,
                        ]));
                    }
                    _ => todo!("{:?}", s),
                },
                Event::Eof => {
                    break Err(quick_xml::Error::UnexpectedEof("Expect property".into()).into())
                }
                _ => {}
            }
        }
    }

    fn read_match<B: BufRead>(&mut self, reader: &mut Reader<B>) -> Result<Match> {
        let mut ret = Match::default();

        loop {
            match reader.read_event(&mut self.buf)? {
                Event::Text(_) | Event::Comment(_) => continue,
                Event::Start(s) => match s.name() {
                    b"test" => {
                        let mut test = Test::default();
                        let mut name = PropertyKind::default();

                        for attr in s.attributes() {
                            let attr = attr?;
                            match attr.key {
                                b"name" => name = attr.parse(reader)?,
                                b"qual" => test.qual = attr.parse(reader)?,
                                b"target" => test.target = attr.parse(reader)?,
                                b"compare" => test.compare = attr.parse(reader)?,
                                _ => {}
                            }
                        }

                        test.value = name.make_property(self.read_value(reader)?)?;
                        reader.read_to_end(b"test", &mut self.buf)?;

                        ret.tests.push(test);
                    }
                    b"edit" => {
                        let mut edit = Edit::default();
                        let mut name = PropertyKind::default();

                        for attr in s.attributes() {
                            let attr = attr?;

                            match attr.key {
                                b"name" => name = attr.parse(reader)?,
                                b"mode" => edit.mode = attr.parse(reader)?,
                                b"binding" => edit.binding = attr.parse(reader)?,
                                _ => {}
                            }
                        }

                        edit.value = name.make_property(self.read_value(reader)?)?;
                        reader.read_to_end(b"edit", &mut self.buf)?;

                        ret.edits.push(edit);
                    }
                    _ => {}
                },
                Event::End(e) => {
                    if e.name() == b"match" {
                        break;
                    }
                }
                Event::Eof => break,
                _ => {}
            }
        }

        Ok(ret)
    }

    fn read_config<B: BufRead>(&mut self, reader: &mut Reader<B>) -> Result<Config> {
        let mut ret = Config::default();

        loop {
            match reader.read_event(&mut self.buf)? {
                Event::Start(s) => match s.name() {
                    b"rescan" => {
                        let n = self.read_string(b"int", reader)?.parse()?;
                        ret.rescans.push(n);
                    }
                    _ => {}
                },
                Event::End(e) => {
                    if e.name() == b"config" {
                        break Ok(ret);
                    }
                }
                Event::Eof => {
                    break Err(Error::Xml(quick_xml::Error::UnexpectedEof(format!(
                        "Expected config"
                    ))))
                }
                _ => {}
            }
        }
    }

    pub fn read_document<B: BufRead>(&mut self, reader: &mut Reader<B>) -> Result<Document> {
        self.clear();

        // STAGE 1. validate document

        loop {
            match reader.read_event(&mut self.buf)? {
                Event::Decl(_) | Event::Text(_) | Event::Comment(_) => continue,
                Event::DocType(doc_type) => {
                    if doc_type.as_ref() != b" fontconfig SYSTEM \"urn:fontconfig:fonts.dtd\"" {
                        return Err(Error::UnmatchedDocType);
                    }
                }
                Event::Start(s) => {
                    if s.name() == b"fontconfig" {
                        break;
                    }
                }
                _ => return Err(Error::NoFontconfig),
            }
        }

        let mut ret = Document::default();

        // STAGE 2. read elements

        loop {
            match reader.read_event(&mut self.buf)? {
                Event::Start(s) => match s.name() {
                    b"description" => {
                        ret.description = reader.read_text(b"description", &mut self.buf)?;
                    }
                    b"match" => {
                        ret.matches.push(self.read_match(reader)?);
                    }
                    b"config" => {
                        ret.config = self.read_config(reader)?;
                    }
                    b"dir" => {
                        let mut dir = Dir::default();

                        for attr in s.attributes() {
                            let attr = attr?;

                            match attr.key {
                                b"prefix" => {
                                    dir.prefix = attr.parse(reader)?;
                                }
                                b"salt" => {
                                    dir.salt = Some(attr.unescape_and_decode_value(reader)?);
                                }
                                _ => {}
                            }
                        }

                        dir.path = reader.read_text(b"dir", &mut self.buf)?;

                        ret.dirs.push(dir);
                    }
                    b"cachedir" => {
                        let mut dir = CacheDir::default();

                        for attr in s.attributes() {
                            let attr = attr?;

                            match attr.key {
                                b"prefix" => {
                                    dir.prefix = attr.parse(reader)?;
                                }
                                _ => {}
                            }
                        }

                        dir.path = reader.read_text(b"cachedir", &mut self.buf)?;

                        ret.cache_dirs.push(dir);
                    }
                    b"include" => {
                        let mut dir = Include::default();

                        for attr in s.attributes() {
                            let attr = attr?;

                            match attr.key {
                                b"prefix" => {
                                    dir.prefix = attr.parse(reader)?;
                                }
                                b"ignore_missing" => match attr.unescaped_value()?.as_ref() {
                                    b"yes" => {
                                        dir.ignore_missing = true;
                                    }
                                    b"no" => {
                                        dir.ignore_missing = false;
                                    }
                                    _ => {
                                        return Err(Error::InvalidFormat);
                                    }
                                },
                                _ => {}
                            }
                        }

                        dir.path = reader.read_text(b"include", &mut self.buf)?;

                        ret.includes.push(dir);
                    }
                    _ => {
                        eprintln!("Unknown element: {:?}", s);
                    }
                },
                Event::Eof => break,
                _ => {}
            }
        }

        Ok(ret)
    }
}

#[cfg(test)]
mod tests {
    use crate::*;

    #[test]
    fn it_works() {
        let mut doc_reader = DocumentReader::new();
        let doc = doc_reader
            .read_document(&mut quick_xml::Reader::from_str(include_str!(
                "/etc/fonts/fonts.conf"
            )))
            .unwrap();

        dbg!(doc);
    }
}
