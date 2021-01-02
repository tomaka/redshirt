// Copyright (C) 2019-2021  Pierre Krieger
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

//! The compositor is responsible for gathering all video outputs, mouse inputs, keyboard inputs,
//! and framebuffers, and mix them together.
//!
//!                       +--------------------------------------------+
//!                       |                                            |
//!                 <-->  |                                            |  +-->
//!                       |                                            |
//!                 <-->  |     +================================>     |  +-->
//!                       |                                            |
//!   framebuffer   <-->  |                                            |  +-->    video output
//!    interface          |                 compositor                 |           interface
//!                 <-->  |                                            |  +-->
//!                       |                                            |
//!                 <-->  |     <================+                     |  +-->
//!                       |                      |                     |
//!                 <-->  |                      +                     |  +-->
//!                       |                                            |
//!                       +--------------------------------------------+
//!
//!                                    ^    ^    ^    ^    ^
//!                                    |    |    |    |    |
//!                                    +    +    +    +    +
//!
//!                                human-input-related interfaces
//!                                (e.g. mouse, keyboard, touch)
//!
//!

// TODO: this entire module is a stub right now
