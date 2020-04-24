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

use std::{error, path::Path};

/// Clones the given repositories and runs `git pull` from time to time.
pub fn clone_git_repos(
    git_urls: impl IntoIterator<Item = impl AsRef<str>>,
) -> Result<GitClones, Box<dyn error::Error + Send + Sync>> {
    clone_git_repos_inner(git_urls)
}

/// Holds a list of clones repositories.
pub struct GitClones {
    paths: Vec<tempdir::TempDir>,
}

impl GitClones {
    /// Returns the list of paths to git repositories that can be watched.
    ///
    /// If the [`GitClones`] object is dropped, these paths are no longer valid.
    pub fn paths(&self) -> impl ExactSizeIterator<Item = &Path> {
        self.paths.iter().map(|p| p.path())
    }
}

#[cfg(feature = "git")]
fn clone_git_repos_inner(
    git_urls: impl IntoIterator<Item = impl AsRef<str>>,
) -> Result<GitClones, Box<dyn error::Error + Send + Sync>> {
    let mut dirs = Vec::new();
    let mut repositories = Vec::new();

    for git_url in git_urls {
        let target_dir = tempdir::TempDir::new("redshirt")?;
        let repo = git2::Repository::clone(git_url.as_ref(), target_dir.path())?;
        dirs.push(target_dir);
        repositories.push(repo);
    }

    // Spawns a thread that updates git repos every minute.
    // TODO: have some way to shut down this thread
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(60));

        for repo in &mut repositories {
            repo.find_remote("origin")
                .unwrap()
                .fetch(&["master"], None, None)
                .unwrap();
            repo.set_head("FETCH_HEAD").unwrap();
            repo.checkout_head(Some(git2::build::CheckoutBuilder::default().force()))
                .unwrap();
        }
    });

    Ok(GitClones { paths: dirs })
}

#[cfg(not(feature = "git"))]
fn clone_git_repos_inner(
    iter: impl IntoIterator<Item = impl AsRef<str>>,
) -> Result<GitClones, Box<dyn error::Error + Send + Sync>> {
    let mut iter = iter.into_iter();
    if iter.next().is_some() {
        panic!("The git feature is not enabled")
    }
    Ok(GitClones { paths: Vec::new() })
}
