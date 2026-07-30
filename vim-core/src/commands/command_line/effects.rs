//! Pure effect builders for command line editing.

use crate::effects::{Effect, Effects};
use crate::grammar::CommandLineEdit;

/// Create an effect to edit the command line.
#[inline]
pub fn edit(edit: CommandLineEdit) -> Effects {
    Effects::single(Effect::command_line_edit(edit))
}
