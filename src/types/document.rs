use crate::parser::parse_config;
use crate::*;

use std::collections::BinaryHeap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ConfigPart {
    Description(String),
    SelectFont(SelectFont),
    Dir(Dir),
    CacheDir(CacheDir),
    Include(Include),
    Match(Match),
    Config(Config),
    Alias(Alias),
    RemapDir(RemapDir),
    ResetDirs,
}

#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FontConfig {
    pub select_fonts: Vec<SelectFont>,
    pub dirs: Vec<DirData>,
    pub cache_dirs: Vec<PathBuf>,
    pub remap_dirs: Vec<RemapDirData>,
    pub matches: Vec<Match>,
    pub config: Config,
    pub aliases: Vec<Alias>,
}

impl FontConfig {
    pub fn merge_config<P: AsRef<Path> + ?Sized>(&mut self, config_path: &P) -> Result<()> {
        let config = fs::read_to_string(config_path.as_ref())?;
        let xml_doc = roxmltree::Document::parse(&config)?;

        for part in parse_config(&xml_doc)? {
            match part? {
                ConfigPart::Alias(alias) => self.aliases.push(alias),
                ConfigPart::Config(mut c) => {
                    self.config.rescans.append(&mut c.rescans);
                    self.config.blanks.append(&mut c.blanks);
                }
                ConfigPart::Description(_) => {}
                ConfigPart::Dir(dir) => self.dirs.push(DirData {
                    path: dir.calculate_path(config_path),
                    salt: dir.salt,
                }),
                ConfigPart::CacheDir(dir) => self.cache_dirs.push(dir.calculate_path(config_path)),
                ConfigPart::Match(m) => self.matches.push(m),
                ConfigPart::ResetDirs => self.dirs.clear(),
                ConfigPart::SelectFont(s) => self.select_fonts.push(s),
                ConfigPart::RemapDir(remap) => self.remap_dirs.push(RemapDirData {
                    path: remap.calculate_path(config_path),
                    salt: remap.salt,
                    as_path: remap.as_path,
                }),
                ConfigPart::Include(dir) => {
                    let include_path = dir.calculate_path(config_path);

                    match self.include(&include_path) {
                        Ok(_) => {}
                        Err(err) => {
                            if !dir.ignore_missing {
                                eprintln!("Failed to load {}: {}", include_path.display(), err);
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn include(&mut self, include_path: &Path) -> Result<()> {
        let meta = fs::metadata(include_path)?;
        let ty = meta.file_type();

        // fs::metadata follow symlink so ty is never symlink
        if ty.is_file() {
            self.merge_config(include_path)?;
        } else if ty.is_dir() {
            let dir = std::fs::read_dir(include_path)?;
            let config_paths = dir
                .filter_map(|entry| {
                    let entry = entry.ok()?;
                    let ty = entry.file_type().ok()?;

                    if ty.is_file() || ty.is_symlink() {
                        Some(entry.path())
                    } else {
                        None
                    }
                })
                .collect::<BinaryHeap<_>>();

            for config_path in config_paths {
                // log error?
                self.merge_config(&config_path).ok();
            }
        }

        Ok(())
    }
}

macro_rules! define_config_part_from {
	($($f:ident,)+) => {
        $(
            impl From<$f> for ConfigPart {
                fn from(v: $f) -> Self {
                    ConfigPart::$f(v)
                }
            }
        )+
	};
}

define_config_part_from! {
    SelectFont,
    Dir,
    CacheDir,
    Include,
    Match,
    Config,
    Alias,
    RemapDir,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
/// Final dir data
pub struct DirData {
    /// dir path
    pub path: PathBuf,
    /// 'salt' property affects to determine cache filename. this is useful for example when having different fonts sets on same path at container and share fonts from host on different font path.
    pub salt: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
/// Final remap-dirs data
pub struct RemapDirData {
    /// dir path will be mapped as the path [`as-path`](Self::as_path) in cached information. This is useful if the directory name is an alias (via a bind mount or symlink) to another directory in the system for which cached font information is likely to exist.
    pub path: PathBuf,
    /// 'salt' property affects to determine cache filename. this is useful for example when having different fonts sets on same path at container and share fonts from host on different font path.
    pub salt: String,
    // remapped path
    pub as_path: String,
}
