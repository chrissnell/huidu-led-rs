//! `GetFiles` / `DeleteFiles` — the device's stored-file table.

use super::{parse_int_or, SdkMessage, SdkReplyBody};
use crate::error::ProtoError;
use crate::sdk::envelope::SdkReply;
use crate::sdk::method::SdkMethod;
use crate::sdk::xml::{self, XmlWriter};

/// Body-less `GetFiles` request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GetFiles;

impl SdkMessage for GetFiles {
    const METHOD: SdkMethod = SdkMethod::GetFiles;
    fn parse_body(_reply: &SdkReply) -> Result<Self, ProtoError> {
        Ok(GetFiles)
    }
}

/// One `<file>` entry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileInfo {
    pub name: String,
    pub size: i64,
    /// Bytes already on the device (nonzero mid-upload).
    pub exist_size: i64,
    pub md5: String,
    /// Firmware's file-type tag.
    pub file_type: String,
}

/// The `GetFiles` reply: the device's file listing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileList {
    pub files: Vec<FileInfo>,
}

impl SdkReplyBody for FileList {
    const METHOD: SdkMethod = SdkMethod::GetFiles;

    fn write_body(&self, x: &mut XmlWriter) -> Result<(), ProtoError> {
        x.open("files", &[])?;
        for f in &self.files {
            let (size, exist) = (f.size.to_string(), f.exist_size.to_string());
            x.empty(
                "file",
                &[
                    ("name", &f.name),
                    ("size", &size),
                    ("existSize", &exist),
                    ("md5", &f.md5),
                    ("type", &f.file_type),
                ],
            )?;
        }
        x.close("files")?;
        Ok(())
    }

    fn parse_body(reply: &SdkReply) -> Result<Self, ProtoError> {
        let mut list = FileList::default();
        xml::elements(&reply.raw, |e| {
            if xml::local_name(e) == b"file" {
                let mut f = FileInfo::default();
                if let Some(v) = xml::attr(e, "name")? {
                    f.name = v;
                }
                if let Some(v) = xml::attr(e, "size")? {
                    f.size = parse_int_or(&v, 0);
                }
                if let Some(v) = xml::attr(e, "existSize")? {
                    f.exist_size = parse_int_or(&v, 0);
                }
                if let Some(v) = xml::attr(e, "md5")? {
                    f.md5 = v;
                }
                if let Some(v) = xml::attr(e, "type")? {
                    f.file_type = v;
                }
                list.files.push(f);
            }
            Ok(())
        })?;
        Ok(list)
    }
}

/// The `DeleteFiles` request: names to remove.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeleteFiles {
    pub names: Vec<String>,
}

impl DeleteFiles {
    /// Build a delete request from a set of file names.
    pub fn new(names: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            names: names.into_iter().map(Into::into).collect(),
        }
    }
}

impl SdkMessage for DeleteFiles {
    const METHOD: SdkMethod = SdkMethod::DeleteFiles;

    fn write_body(&self, x: &mut XmlWriter) -> Result<(), ProtoError> {
        x.open("files", &[])?;
        for name in &self.names {
            x.empty("file", &[("name", name)])?;
        }
        x.close("files")?;
        Ok(())
    }

    fn parse_body(reply: &SdkReply) -> Result<Self, ProtoError> {
        let mut names = Vec::new();
        xml::elements(&reply.raw, |e| {
            if xml::local_name(e) == b"file" {
                if let Some(v) = xml::attr(e, "name")? {
                    names.push(v);
                }
            }
            Ok(())
        })?;
        Ok(DeleteFiles { names })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sdk::envelope::encode_reply;
    use crate::sdk::result::SdkResult;

    #[test]
    fn get_request_round_trips() {
        let bytes = GetFiles.encode_request("g").unwrap();
        assert_eq!(GetFiles::decode(&bytes).unwrap(), GetFiles);
    }

    #[test]
    fn file_list_reply_round_trips() {
        let list = FileList {
            files: vec![
                FileInfo {
                    name: "logo.png".into(),
                    size: 4096,
                    exist_size: 4096,
                    md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
                    file_type: "image".into(),
                },
                FileInfo {
                    name: "clip.mp4".into(),
                    size: 1_048_576,
                    exist_size: 0,
                    md5: "abc123".into(),
                    file_type: "video".into(),
                },
            ],
        };
        let bytes = encode_reply("g", SdkMethod::GetFiles, &SdkResult::success(), |x| {
            list.write_body(x)
        })
        .unwrap();
        assert_eq!(FileList::decode(&bytes).unwrap(), list);
    }

    #[test]
    fn delete_request_round_trips() {
        let req = DeleteFiles::new(["a.jpg", "b.mp4"]);
        let bytes = req.encode_request("g").unwrap();
        assert_eq!(DeleteFiles::decode(&bytes).unwrap(), req);
    }
}
