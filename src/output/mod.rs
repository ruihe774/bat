use std::error::Error as StdError;
use std::fmt::{self, Display};
use std::io::{self, Write};
use std::mem;
#[cfg(feature = "paging")]
use std::process::{Child, ChildStdin};

use crate::config::ConsolidatedConfig as Config;
use crate::error::Result;
#[cfg(feature = "paging")]
use less::{retrieve_less_version, LessVersion};
pub use pager::PagingMode;

#[cfg(feature = "paging")]
mod less;
pub(crate) mod pager;

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
    Stdout(io::StdoutLock<'static>),
}

impl OutputType {
    #[cfg(feature = "paging")]
    pub fn from_mode(paging_mode: PagingMode, config: &Config, panel_width: usize) -> Result<Self> {
        Ok(match paging_mode {
            PagingMode::Always => {
                OutputType::try_pager(SingleScreenAction::Nothing, config, panel_width)?
            }
            PagingMode::QuitIfOneScreen => {
                OutputType::try_pager(SingleScreenAction::Quit, config, panel_width)?
            }
            PagingMode::Never => OutputType::stdout(),
        })
    }

    /// Try to launch the pager. Fall back to stdout in case of errors.
    #[cfg(feature = "paging")]
    fn try_pager(
        single_screen_action: SingleScreenAction,
        config: &Config,
        panel_width: usize,
    ) -> Result<Self> {
        use pager::{PagerKind, PagerSource};
        use std::process::{Command, Stdio};

        let pager_opt = pager::get_pager(config.pager.as_deref())?;

        let Some(pager) = pager_opt else {
            return Ok(OutputType::stdout());
        };

        if pager.kind == PagerKind::Bat {
            return Err(InvalidPagerValueBat.into());
        }

        let Ok(resolved_path) = grep_cli::resolve_binary(&pager.bin) else {
            return Ok(OutputType::stdout());
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
            let less_version = match retrieve_less_version(&pager.bin) {
                None => 1,
                Some(LessVersion::Less(version)) => version,
                _ => 0,
            };

            if args.is_empty() || replace_arguments_to_less {
                p.arg("-R"); // Short version of --RAW-CONTROL-CHARS for maximum compatibility

                if single_screen_action == SingleScreenAction::Quit {
                    p.arg("-F"); // Short version of --quit-if-one-screen for compatibility
                }

                // Passing '--no-init' fixes a bug with '--quit-if-one-screen' in older
                // versions of 'less'. Unfortunately, it also breaks mouse-wheel support.
                //
                // See: http://www.greenwoodsoftware.com/less/news.530.html
                //
                // For newer versions (530 or 558 on Windows), we omit '--no-init' as it
                // is not needed anymore.
                if less_version == 1
                    || (less_version > 1
                        && (less_version < 530 || (cfg!(windows) && less_version < 558)))
                {
                    p.arg("--no-init");
                }

                if less_version >= 600 {
                    let mut col_header = 0;
                    let have_numbers = config.style_components.numbers();

                    if have_numbers && panel_width > 0 {
                        col_header += panel_width;
                    }

                    if col_header > 0 {
                        p.arg("--header");
                        p.arg(format!("0,{col_header}"));
                        p.arg("--no-search-headers");
                    }
                }
            } else {
                p.args(args);
            }

            p.env("LESSCHARSET", "UTF-8");

            #[cfg(feature = "lessopen")]
            // Ensures that 'less' does not preprocess input again if '$LESSOPEN' is set.
            {
                p.env_remove("LESSOPEN");
                p.env_remove("LESSCLOSE");
            }
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
            .map_or_else(OutputType::stdout, |(child, stdin)| {
                OutputType::Pager(child, Some(io::LineWriter::new(stdin)))
            }))
    }

    pub fn stdout() -> Self {
        OutputType::Stdout(io::stdout().lock())
    }

    #[cfg(feature = "paging")]
    pub fn is_pager(&self) -> bool {
        matches!(self, OutputType::Pager(_, _))
    }

    #[cfg(not(feature = "paging"))]
    pub fn is_pager(&self) -> bool {
        false
    }

    pub fn is_stdout(&self) -> bool {
        matches!(self, OutputType::Stdout(_))
    }

    pub fn stdout_handle(&mut self) -> Option<&mut impl Write> {
        match self {
            OutputType::Stdout(handle) => Some(handle),
            OutputType::Pager(_, _) => None,
        }
    }

    pub fn pager_handle(&mut self) -> Option<&mut impl Write> {
        #[cfg(feature = "paging")]
        match self {
            OutputType::Pager(_, handle) => Some(handle.as_mut().unwrap()),
            OutputType::Stdout(_) => None,
        }
        #[cfg(not(feature = "paging"))]
        None
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
