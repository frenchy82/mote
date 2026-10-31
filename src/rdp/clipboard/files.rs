//! Files on the clipboard: the local ones described to the server and read
//! for it piece by piece, and the remote ones fetched into a directory here.

use std::io::{Read as _, Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use ironrdp_cliprdr::pdu::{
    ClipboardFileAttributes, FileContentsFlags, FileContentsRequest, FileContentsResponse,
    FileDescriptor,
};

/// How many files and folders one copy offers at most.
const MAX_FILES: usize = 10_000;
/// The most read for one request, whatever the server asks for.
const MAX_READ: u32 = 8 << 20;
/// The piece asked for at a time when fetching.
const CHUNK: u32 = 1 << 20;
/// Seconds between 1601, where Windows counts from, and 1970.
const WINDOWS_EPOCH: u64 = 11_644_473_600;

/// The longest name the file list carries, with its folders.
const MAX_NAME: usize = 259;

/// A local file or folder offered to the server, in the order of the list
/// sent, which is how the server names it.
pub(super) struct Offered {
    path: PathBuf,
    size: u64,
}

/// The file list for `paths`, folders with all they hold, and what each
/// entry is.
pub(super) fn describe(paths: &[PathBuf]) -> (Vec<FileDescriptor>, Vec<Offered>) {
    let mut list = Vec::new();
    let mut offered = Vec::new();
    for path in paths {
        add(path, None, &mut list, &mut offered);
    }
    (list, offered)
}

fn add(
    path: &Path,
    parent: Option<&str>,
    list: &mut Vec<FileDescriptor>,
    offered: &mut Vec<Offered>,
) {
    if list.len() >= MAX_FILES {
        return;
    }
    let (Some(name), Ok(meta)) = (
        path.file_name().map(|n| n.to_string_lossy().into_owned()),
        std::fs::metadata(path),
    ) else {
        return;
    };
    let wire = match parent {
        Some(parent) => parent.chars().count() + 1 + name.chars().count(),
        None => name.chars().count(),
    };
    // IronRDP leaves such names out of the list, which would put every
    // later entry at the wrong index.
    if name.is_empty() || wire > MAX_NAME {
        tracing::debug!("not offering {}: its name is too long", path.display());
        return;
    }
    let mut descriptor = FileDescriptor::new(&name);
    if let Some(parent) = parent {
        descriptor = descriptor.with_relative_path(parent);
    }
    if let Some(time) = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
    {
        let ticks =
            (time.as_secs() + WINDOWS_EPOCH) * 10_000_000 + u64::from(time.subsec_nanos() / 100);
        descriptor = descriptor.with_last_write_time(ticks);
    }
    if meta.is_dir() {
        list.push(descriptor.with_attributes(ClipboardFileAttributes::DIRECTORY));
        offered.push(Offered {
            path: path.to_owned(),
            size: 0,
        });
        let here = match parent {
            Some(parent) => format!("{parent}\\{name}"),
            None => name,
        };
        let Ok(entries) = std::fs::read_dir(path) else {
            return;
        };
        let mut children: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        children.sort();
        for child in children {
            add(&child, Some(&here), list, offered);
        }
    } else if meta.is_file() {
        list.push(
            descriptor
                .with_attributes(ClipboardFileAttributes::ARCHIVE)
                .with_file_size(meta.len()),
        );
        offered.push(Offered {
            path: path.to_owned(),
            size: meta.len(),
        });
    }
}

/// The server's request for a local file's size or a piece of it.
pub(super) fn serve(
    offered: &[Offered],
    request: &FileContentsRequest,
) -> FileContentsResponse<'static> {
    let stream = request.stream_id;
    let Some(file) = usize::try_from(request.index)
        .ok()
        .and_then(|i| offered.get(i))
    else {
        return FileContentsResponse::new_error(stream);
    };
    if request.flags.contains(FileContentsFlags::SIZE) {
        return FileContentsResponse::new_size_response(stream, file.size);
    }
    let read = || -> std::io::Result<Vec<u8>> {
        let mut handle = std::fs::File::open(&file.path)?;
        handle.seek(SeekFrom::Start(request.position))?;
        let mut data = Vec::new();
        handle
            .take(u64::from(request.requested_size.min(MAX_READ)))
            .read_to_end(&mut data)?;
        Ok(data)
    };
    match read() {
        Ok(data) => FileContentsResponse::new_data_response(stream, data),
        Err(e) => {
            tracing::warn!("reading {} for the server: {e}", file.path.display());
            FileContentsResponse::new_error(stream)
        }
    }
}

/// The remote files being fetched into a directory, one piece at a time.
pub(super) struct Fetch {
    dir: PathBuf,
    files: Vec<FileDescriptor>,
    lock: Option<u32>,
    stream: u32,
    /// The entry being fetched.
    index: usize,
    out: Option<std::fs::File>,
    position: u64,
}

/// What a fetch wants next.
pub(super) enum Step {
    Request(FileContentsRequest),
    /// All here: what is at the top of the directory.
    Done(Vec<PathBuf>),
    Failed,
}

impl Fetch {
    pub(super) fn new(
        dir: PathBuf,
        files: &[FileDescriptor],
        lock: Option<u32>,
        stream: u32,
    ) -> Self {
        Self {
            dir,
            files: files.to_vec(),
            lock,
            stream,
            index: 0,
            out: None,
            position: 0,
        }
    }

    pub(super) fn stream(&self) -> u32 {
        self.stream
    }

    /// Where an entry goes. IronRDP has already turned away names that
    /// climb out of the directory.
    fn path(&self, file: &FileDescriptor) -> PathBuf {
        let mut path = self.dir.clone();
        for part in file
            .relative_path
            .iter()
            .flat_map(|p| p.split(['\\', '/']))
            .filter(|p| !p.is_empty() && *p != "." && *p != "..")
        {
            path.push(part);
        }
        path.push(file.name.replace('/', "_"));
        path
    }

    /// Start, or go on to the next file.
    pub(super) fn next(&mut self) -> Step {
        while let Some(file) = self.files.get(self.index) {
            let path = self.path(file);
            let made = if file
                .attributes
                .is_some_and(|a| a.contains(ClipboardFileAttributes::DIRECTORY))
            {
                std::fs::create_dir_all(&path).map(|()| None)
            } else {
                path.parent()
                    .map_or(Ok(()), std::fs::create_dir_all)
                    .and_then(|()| std::fs::File::create(&path))
                    .map(Some)
            };
            match made {
                Ok(None) => self.index += 1,
                Ok(Some(_)) if file.file_size == Some(0) => self.index += 1,
                Ok(Some(out)) => {
                    self.out = Some(out);
                    self.position = 0;
                    return Step::Request(self.request());
                }
                Err(e) => {
                    tracing::warn!("fetching into {}: {e}", path.display());
                    return Step::Failed;
                }
            }
        }
        let top = self
            .files
            .iter()
            .filter(|f| f.relative_path.as_deref().is_none_or(str::is_empty))
            .map(|f| self.path(f))
            .collect();
        Step::Done(top)
    }

    fn request(&self) -> FileContentsRequest {
        // IronRDP turns down a request past the end of a file it knows the
        // size of.
        let left = self.files[self.index]
            .file_size
            .map_or(u64::from(CHUNK), |size| size.saturating_sub(self.position));
        FileContentsRequest {
            stream_id: self.stream,
            index: self.index as i32,
            flags: FileContentsFlags::RANGE,
            position: self.position,
            requested_size: left.min(u64::from(CHUNK)) as u32,
            data_id: self.lock,
        }
    }

    /// A piece arrived: write it, and ask for the next.
    pub(super) fn received(&mut self, response: &FileContentsResponse<'_>) -> Step {
        if response.is_error() {
            return Step::Failed;
        }
        let data = response.data();
        let Some(out) = self.out.as_mut() else {
            return Step::Failed;
        };
        if let Err(e) = out.write_all(data) {
            tracing::warn!("writing a fetched file: {e}");
            return Step::Failed;
        }
        self.position += data.len() as u64;
        let size = self.files[self.index].file_size;
        // A short piece, or an empty one, is the end where no size was given.
        let more = match size {
            Some(size) => self.position < size && !data.is_empty(),
            None => data.len() as u64 == u64::from(CHUNK),
        };
        if more {
            return Step::Request(self.request());
        }
        self.out = None;
        self.index += 1;
        self.next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_is_offered_with_what_it_holds() {
        let dir = std::env::temp_dir().join(format!("mote-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("folder/inner")).unwrap();
        std::fs::write(dir.join("folder/a.txt"), b"hello").unwrap();
        std::fs::write(dir.join("folder/inner/b.txt"), b"there").unwrap();
        let (list, offered) = describe(&[dir.join("folder")]);
        let names: Vec<_> = list
            .iter()
            .map(|f| (f.relative_path.clone(), f.name.clone()))
            .collect();
        assert_eq!(
            names,
            vec![
                (None, "folder".to_owned()),
                (Some("folder".to_owned()), "a.txt".to_owned()),
                (Some("folder".to_owned()), "inner".to_owned()),
                (Some("folder\\inner".to_owned()), "b.txt".to_owned()),
            ]
        );
        let request = FileContentsRequest {
            stream_id: 7,
            index: 1,
            flags: FileContentsFlags::RANGE,
            position: 1,
            requested_size: 3,
            data_id: None,
        };
        assert_eq!(serve(&offered, &request).data(), b"ell");

        // Fetched back into another directory, the same tree comes out.
        let into = dir.join("fetched");
        let mut fetch = Fetch::new(into.clone(), &list, None, 1);
        let mut step = fetch.next();
        loop {
            match step {
                Step::Request(request) => {
                    let response = serve(&offered, &request);
                    step = fetch.received(&response);
                }
                Step::Done(top) => {
                    assert_eq!(top, vec![into.join("folder")]);
                    break;
                }
                Step::Failed => panic!("the fetch failed"),
            }
        }
        assert_eq!(
            std::fs::read(into.join("folder/inner/b.txt")).unwrap(),
            b"there"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
