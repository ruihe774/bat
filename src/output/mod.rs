use std::error::Error as StdError;
use std::fmt::{self, Display};
use std::io::{self, Write};
use std::mem;
#[cfg(feature = "paging")]
use std::process::{Child, ChildStdin};

use crate::error::*;
use crate::printer::WrappingMode;
#[cfg(feature = "paging")]
use less::{retrieve_less_version, LessVersion};
use pager::PagingMode;

#[cfg(feature = "paging")]
mod less;
pub mod pager;

#[derive(Debug)]
pub struct InvalidPagerValueBat;

impl Display for InvalidPagerValueBat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "use of bat as a pager is disallowed to avoid infinite recursion"
        )
    }
}

impl StdError for InvalidPagerValueBat {}

#[cfg(feature = "paging")]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum SingleScreenAction {
    Quit,
    Nothing,
}

#[derive(Debug)]
pub(crate) enum OutputType {
    #[cfg(feature = "paging")]
    Pager(Child, Option<io::LineWriter<ChildStdin>>),
    Stdout(io::Stdout),
}

impl OutputType {
    #[cfg(feature = "paging")]
    pub fn from_mode(
        paging_mode: PagingMode,
        wrapping_mode: WrappingMode,
        pager: Option<&str>,
    ) -> Result<Self> {
        Ok(match paging_mode {
            PagingMode::Always => {
                OutputType::try_pager(SingleScreenAction::Nothing, wrapping_mode, pager)?
            }
            PagingMode::QuitIfOneScreen => {
                OutputType::try_pager(SingleScreenAction::Quit, wrapping_mode, pager)?
            }
            _ => OutputType::stdout(),
        })
    }

    /// Try to launch the pager. Fall back to stdout in case of errors.
    #[cfg(feature = "paging")]
    fn try_pager(
        single_screen_action: SingleScreenAction,
        wrapping_mode: WrappingMode,
        pager_from_config: Option<&str>,
    ) -> Result<Self> {
        use pager::{PagerKind, PagerSource};
        use std::process::{Command, Stdio};

        let pager_opt = pager::get_pager(pager_from_config)?;

        let pager = match pager_opt {
            Some(pager) => pager,
            None => return Ok(OutputType::stdout()),
        };

        if pager.kind == PagerKind::Bat {
            return Err(InvalidPagerValueBat.into());
        }

        let resolved_path = match grep_cli::resolve_binary(&pager.bin) {
            Ok(path) => path,
            Err(_) => {
                return Ok(OutputType::stdout());
            }
        };

        let mut p = Command::new(resolved_path);
        let args = pager.args;

        if pager.kind == PagerKind::Less {
            // less needs to be called with the '-R' option in order to properly interpret the
            // ANSI color sequences printed by bat. If someone has set PAGER="less -F", we
            // therefore need to overwrite the arguments and add '-R'.
            //
            // We only do this for PAGER (as it is not specific to 'bat'), not for BAT_PAGER
            // or bats '--pager' command line option.
            let replace_arguments_to_less = pager.source == PagerSource::EnvVarPager;

            if args.is_empty() || replace_arguments_to_less {
                p.arg("-R"); // Short version of --RAW-CONTROL-CHARS for maximum compatibility
                if single_screen_action == SingleScreenAction::Quit {
                    p.arg("-F"); // Short version of --quit-if-one-screen for compatibility
                }

                if wrapping_mode == WrappingMode::NoWrapping(true) {
                    p.arg("-S"); // Short version of --chop-long-lines for compatibility
                }

                // Passing '--no-init' fixes a bug with '--quit-if-one-screen' in older
                // versions of 'less'. Unfortunately, it also breaks mouse-wheel support.
                //
                // See: http://www.greenwoodsoftware.com/less/news.530.html
                //
                // For newer versions (530 or 558 on Windows), we omit '--no-init' as it
                // is not needed anymore.
                match retrieve_less_version(&pager.bin) {
                    None => {
                        p.arg("--no-init");
                    }
                    Some(LessVersion::Less(version))
                        if (version < 530 || (cfg!(windows) && version < 558)) =>
                    {
                        p.arg("--no-init");
                    }
                    _ => {}
                }
            } else {
                p.args(args);
            }
            p.env("LESSCHARSET", "UTF-8");

            #[cfg(feature = "lessopen")]
            // Ensures that 'less' does not preprocess input again if '$LESSOPEN' is set.
            p.arg("--no-lessopen");
        } else {
            p.args(args);
        };

        Ok(p.stdin(Stdio::piped())
            .spawn()
            .ok()
            .map(|mut child| {
                let stdin = child.stdin.take();
                (child, stdin)
            })
            .and_then(|(mut child, stdin)| {
                if let Some(stdin) = stdin {
                    Some((child, stdin))
                } else {
                    _ = child.kill();
                    _ = child.wait();
                    None
                }
            })
            .map(|(child, stdin)| OutputType::Pager(child, Some(io::LineWriter::new(stdin))))
            .unwrap_or_else(|| OutputType::stdout()))
    }

    pub fn stdout() -> Self {
        OutputType::Stdout(io::stdout())
    }

    #[cfg(feature = "paging")]
    pub fn is_pager(&self) -> bool {
        matches!(self, OutputType::Pager(_, _))
    }

    #[cfg(not(feature = "paging"))]
    pub fn is_pager(&self) -> bool {
        false
    }

    pub fn handle(&mut self) -> Result<&mut dyn Write> {
        Ok(match self {
            #[cfg(feature = "paging")]
            OutputType::Pager(_, handle) => handle.as_mut().unwrap(),
            OutputType::Stdout(handle) => handle,
        })
    }
}

#[cfg(feature = "paging")]
impl Drop for OutputType {
    fn drop(&mut self) {
        if let OutputType::Pager(child, stdin) = self {
            mem::drop(stdin.take());
            _ = child.wait();
        }
    }
}
