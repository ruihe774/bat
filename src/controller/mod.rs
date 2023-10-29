use std::io::{self, IsTerminal, Write};

use clircle::{Clircle, Identifier};
use nu_ansi_term::Color;

use crate::assets::HighlightingAssets;
use crate::config::ConsolidatedConfig as Config;
use crate::error::*;
use crate::input::{Input, OpenedInput};
use crate::output::OutputType;
use crate::printer::{InteractivePrinter, Printer, SimplePrinter};
use line_range::{LineRanges, RangeCheckResult};

pub mod line_range;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ErrorHandling {
    NoError,
    Handled,
    SilentFail,
}

pub fn default_error_handler(
    error: Error,
    output: &mut dyn Write,
    is_terminal: bool,
) -> ErrorHandling {
    for cause in error.chain() {
        if let Some(io_error) = cause.downcast_ref::<io::Error>() {
            if io_error.kind() == io::ErrorKind::BrokenPipe {
                return ErrorHandling::SilentFail;
            }
        }
    }

    let style = is_terminal.then(|| Color::Red.normal()).unwrap_or_default();
    writeln!(
        output,
        "{}[bat error]{}: {:?}",
        style.prefix(),
        style.suffix(),
        error
    )
    .expect("failed to print error");
    return ErrorHandling::Handled;
}

pub struct Controller<'a> {
    config: &'a Config,
    assets: &'a HighlightingAssets,
}

impl<'a> Controller<'a> {
    pub fn new(config: &'a Config, assets: &'a HighlightingAssets) -> Self {
        Controller { config, assets }
    }

    pub fn run(&self, inputs: Vec<Input>) -> Result<ErrorHandling> {
        self.run_with_options(inputs, Option::<&mut Vec<u8>>::None, default_error_handler)
    }

    pub fn run_with_options(
        &self,
        inputs: Vec<Input>,
        mut output_buffer: Option<&mut impl Write>,
        handle_error: impl Fn(Error, &mut dyn Write, bool) -> ErrorHandling,
    ) -> Result<ErrorHandling> {
        let panel_width = (!self.config.loop_through)
            .then(|| InteractivePrinter::get_panel_width(&self.config))
            .unwrap_or_default();

        #[cfg(feature = "paging")]
        let mut output_type = if output_buffer.is_none() {
            Some(OutputType::from_mode(
                self.config.paging_mode,
                &self.config,
                panel_width,
            )?)
        } else {
            None
        };

        #[cfg(not(feature = "paging"))]
        let mut output_type = output_buffer.is_none().then(|| OutputType::stdout());

        let stdout_identifier = (output_buffer.is_none()
            && !output_type.as_ref().unwrap().is_pager())
        .then(clircle::Identifier::stdout)
        .flatten();

        let mut no_errors: bool = true;
        let mut stderr = io::stderr();

        for (index, input) in inputs.into_iter().enumerate() {
            let identifier = stdout_identifier.as_ref();
            let is_first = index == 0;
            let result = match (output_buffer.as_mut(), output_type.as_mut()) {
                (Some(buffer), None) => self.print_input(input, *buffer, identifier, is_first),
                (None, Some(output_type)) if output_type.is_pager() => self.print_input(
                    input,
                    output_type.pager_handle().unwrap(),
                    identifier,
                    is_first,
                ),
                (None, Some(output_type)) if output_type.is_stdout() => self.print_input(
                    input,
                    output_type.stdout_handle().unwrap(),
                    identifier,
                    is_first,
                ),
                _ => unreachable!(),
            };
            if let Err(error) = result {
                if output_buffer.is_some() {
                    // It doesn't make much sense to send errors straight to stderr if the user
                    // provided their own buffer, so we just return it.
                    return Err(error);
                } else {
                    let output_type = output_type.as_mut().unwrap();
                    match if output_type.is_pager() {
                        let is_terminal = io::stdout().is_terminal();
                        handle_error(error, output_type.pager_handle().unwrap(), is_terminal)
                    } else {
                        let is_terminal = stderr.is_terminal();
                        handle_error(error, &mut stderr, is_terminal)
                    } {
                        ErrorHandling::Handled | ErrorHandling::NoError => (),
                        ErrorHandling::SilentFail => return Ok(ErrorHandling::SilentFail),
                    }
                }
                no_errors = false;
            }
        }

        Ok(if no_errors {
            ErrorHandling::NoError
        } else {
            ErrorHandling::Handled
        })
    }

    fn print_input<W: Write>(
        &self,
        input: Input,
        writer: &mut W,
        stdout_identifier: Option<&Identifier>,
        is_first: bool,
    ) -> Result<()> {
        let mut opened_input = input.open(
            stdout_identifier,
            #[cfg(feature = "lessopen")]
            !self.config.no_lessopen,
        )?;

        if self.config.loop_through {
            let mut printer = SimplePrinter::new(&self.config);
            self.print_file(&mut printer, writer, &mut opened_input, !is_first)
        } else {
            let mut printer =
                InteractivePrinter::new(&self.config, self.assets, &mut opened_input)?;
            self.print_file(&mut printer, writer, &mut opened_input, !is_first)
        }
    }

    fn print_file<W: Write>(
        &self,
        printer: &mut impl Printer<W>,
        writer: &mut W,
        input: &mut OpenedInput,
        add_header_padding: bool,
    ) -> Result<()> {
        if input.reader.content_type.is_some() || self.config.style_components.header() {
            printer.print_header(writer, input, add_header_padding)?;
        }

        if input.reader.content_type.is_some() {
            let line_ranges = &self.config.visible_lines.0;
            self.print_file_ranges(printer, writer, input, line_ranges)?;
        }
        printer.print_footer(writer, input)?;

        Ok(())
    }

    fn print_file_ranges<W: Write>(
        &self,
        printer: &mut impl Printer<W>,
        writer: &mut W,
        input: &mut OpenedInput,
        line_ranges: &LineRanges,
    ) -> Result<()> {
        let mut line_buffer = Vec::new();

        let mut first_range: bool = true;
        let mut mid_range: bool = false;

        let style_snip = self.config.style_components.snip();

        for line_number in 1.. {
            let range_check = line_ranges.check(line_number);
            if range_check == RangeCheckResult::AfterLastRange {
                break;
            }

            if !input.reader.read_line(&mut line_buffer).with_context(|| {
                let description = &input.description;
                if let Some(name) = description.name.as_ref() {
                    format!("failed to read '{}'", name.to_string_lossy())
                } else {
                    format!("failed to read {}", description.kind)
                }
            })? {
                break;
            }

            match line_ranges.check(line_number) {
                RangeCheckResult::BeforeOrBetweenRanges => {
                    // Call the printer in case we need to call the syntax highlighter
                    // for this line. However, set `out_of_range` to `true`.
                    printer.print_line(true, writer, line_number, &line_buffer)?;
                    mid_range = false;
                }

                RangeCheckResult::InRange => {
                    if style_snip {
                        if first_range {
                            first_range = false;
                            mid_range = true;
                        } else if !mid_range {
                            mid_range = true;
                            printer.print_snip(writer)?;
                        }
                    }

                    printer.print_line(false, writer, line_number, &line_buffer)?;
                }

                RangeCheckResult::AfterLastRange => unreachable!(),
            }

            line_buffer.clear();
        }

        Ok(())
    }
}
