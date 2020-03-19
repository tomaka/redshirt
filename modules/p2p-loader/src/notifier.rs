// Copyright (C) 2019-2020  Pierre Krieger
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use futures::prelude::*;
use std::{fs, io, path::Path, time::Duration};
use walkdir::WalkDir;

/// Event that the notifier can produce.
#[derive(Debug)]
pub enum NotifierEvent {
    /// Insert a value in the DHT.
    InjectDht {
        /// Key to insert.
        hash: [u8; 32],
        /// Data to insert.
        data: Vec<u8>,
    }, // TODO: more event? remove event?
}

/// Returns a stream of events about the given path in the file system.
pub fn start_notifier(path: impl AsRef<Path>) -> Result<impl Stream<Item = NotifierEvent>, io::Error> {
    start_notifier_inner(path)
}

#[cfg(feature = "notify")]
fn start_notifier_inner(path: impl AsRef<Path>) -> Result<impl Stream<Item = NotifierEvent>, io::Error> {
    use notify::Watcher as _;

    let path = path.as_ref().to_owned();

    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(notify::DebouncedEvent::Rescan).unwrap();
    let (mut async_tx, async_rx) = futures::channel::mpsc::channel(2);

    let mut watcher = notify::watcher(tx, Duration::from_secs(5))
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    watcher
        .watch(&path, notify::RecursiveMode::Recursive)
        .unwrap();

    std::thread::Builder::new()
        .name("files-watcher".to_string())
        .spawn(move || {
            futures::executor::block_on(async move {
                // Make sure that the watcher is kept alive inside the thread.
                let _watcher = watcher;

                loop {
                    let files_to_try = match rx.recv().unwrap() {
                        notify::DebouncedEvent::Write(path)
                        | notify::DebouncedEvent::Create(path) => vec![path],
                        notify::DebouncedEvent::Rescan => {
                            let mut files = Vec::new();
                            for entry in WalkDir::new(&path).into_iter().filter_map(|e| e.ok()) {
                                let path = entry.path();
                                if !path.is_file() {
                                    continue;
                                }
                                files.push(path.to_owned());
                            }
                            files
                        }
                        notify::DebouncedEvent::Error(err, path) => {
                            log::error!("Watcher error: {:?} for {:?}", err, path);
                            continue;
                        }
                        _ => continue,
                    };

                    for path in files_to_try {
                        let data = match fs::read(&path) {
                            Ok(d) => d,
                            Err(err) => {
                                log::warn!("Unable to read content of {}: {}", path.display(), err);
                                continue;
                            }
                        };

                        if !can_be_wasm(&path, &data) {
                            continue;
                        }

                        let hash = blake3::hash(&data);
                        log::info!(
                            "File {:?} has hash {:?}",
                            path,
                            bs58::encode(hash.as_bytes()).into_string()
                        );
                        if async_tx
                            .send(NotifierEvent::InjectDht {
                                hash: *hash.as_bytes(),
                                data,
                            })
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            })
        })?;

    Ok(async_rx)
}

#[cfg(not(feature = "notify"))]
fn start_notifier_inner(_: impl AsRef<Path>) -> Result<futures::stream::Pending<NotifierEvent>, io::Error> {
    panic!("The notify feature is not enabled")
}

/// Returns true if the given file content can potentially be a Wasm file.
///
/// In other words: returns false if we are sure that this isn't a Wasm file, and true
/// otherwise.
#[cfg(feature = "notify")]
fn can_be_wasm(path: impl AsRef<Path>, data: &[u8]) -> bool {
    if path.as_ref().extension() != Some("wasm".as_ref()) {
        return false;
    }

    if data.len() <= 8 {
        return false;
    }

    if &data[0..8] != &[0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00] {
        return false;
    }

    true
}
