// Copyright (C) 2019  Pierre Krieger
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

use regex::Regex;
use std::{env, fs::File, io::Write, path::Path};

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("build-pci.rs");
    let mut f = File::create(&dest_path).unwrap();

    write!(f, r#"
        fn build_pci_info() -> hashbrown::HashMap<(u16, u16), (&'static str, &'static str)> {{
            [
    "#).unwrap();

    let mut current_vendor_id = None::<u16>;
    let mut current_vendor_name = None;

    let vendor_regex = Regex::new(r"^(\w{4})  (.*)$").unwrap();
    let device_regex = Regex::new(r"^\t(\w{4})  (.*)$").unwrap();

    for line in include_str!("build/pci.ids").lines() {
        // Strip comments.
        let line = if let Some(pos) = line.find('#') {
            line.split_at(pos).0
        } else {
            line
        };

        if let Some(regex_match) = device_regex.captures(line) {
            let device_id = u16::from_str_radix(regex_match.get(1).unwrap().as_str(), 16).unwrap();
            let device_name = regex_match.get(2).unwrap().as_str();

            write!(f, r##"
                ((0x{:x}, 0x{:x}), (r#"{}"#, r#"{}"#)),
            "##, current_vendor_id.unwrap(), device_id, current_vendor_name.clone().unwrap(), device_name).unwrap();

        } else if let Some(regex_match) = vendor_regex.captures(line) {
            current_vendor_id = Some(u16::from_str_radix(regex_match.get(1).unwrap().as_str(), 16).unwrap());
            current_vendor_name = Some(regex_match.get(2).unwrap().as_str().to_string());

        } else if !line.is_empty() && !line.starts_with("\t\t") {
            write!(f, r##"
                // Couldn't parse line: {}
            "##, line).unwrap();
        }
    }

    write!(f, r#"
            ].iter().cloned().collect()
        }}
    "#).unwrap();
}
