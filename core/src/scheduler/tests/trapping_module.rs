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

use crate::scheduler::{Core, CoreRunOutcome};

#[test]
fn trapping_module() {
    let module = from_wat!(
        local,
        r#"(module
        (func $_start
            unreachable)
        (export "_start" (func $_start)))
    "#
    );

    let core = Core::new().build();
    let expected_pid = core.execute(&module).unwrap().pid();

    match core.run() {
        CoreRunOutcome::ProgramFinished {
            pid,
            outcome: Err(_),
            ..
        } => {
            assert_eq!(pid, expected_pid);
        }
        _ => panic!(),
    }
}
