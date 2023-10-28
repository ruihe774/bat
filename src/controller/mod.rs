use std::io::{self, Write};
use std::process;

use clircle::{Clircle, Identifier};
use nu_ansi_term::Color;
use serde::{Deserialize, Serialize};

use crate::assets::HighlightingAssets;
use crate::config::Config;
use crate::error::*;
use crate::input::{Input, InputReader, OpenedInput};
#[cfg(feature = "paging")]
use crate::output::pager::PagingMode;
use crate::output::OutputType;
use crate::printer::{InteractivePrinter, OutputHandle, Printer, SimplePrinter};
use line_range::{LineRanges, RangeCheckResult};

pub mod line_range;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VisibleLines(pub LineRanges);

impl Default for VisibleLines {
    fn default() -> Self {
        VisibleLines(LineRanges::all())
    }
}

pub struct Controller<'a> {
    config: &'a Config<'a>,
    assets: &'a HighlightingAssets,
}

pub fn default_error_handler(error: &Error, output: &mut dyn Write) {
    if let Some(io_error) = error.downcast_ref::<io::Error>() {
        if io_error.kind() == io::ErrorKind::BrokenPipe {
            process::exit(0);
        }
    }

    writeln!(output, "{}: {:?}", Color::Red.paint("[bat error]"), error).unwrap();
}

impl<'a> Controller<'a> {
    pub fn new(config: &'a Config, assets: &'a HighlightingAssets) -> Self {
        Controller { config, assets }
    }

    pub fn run(&self, inputs: Vec<Input>, output_buffer: Option<&mut dyn Write>) -> Result<bool> {
        self.run_with_error_handler(inputs, output_buffer, default_error_handler)
    }

    pub fn run_with_error_handler(
        &self,
        inputs: Vec<Input>,
        output_buffer: Option<&mut dyn Write>,
        handle_error: impl Fn(&Error, &mut dyn Write),
    ) -> Result<bool> {
        #[cfg(feature = "paging")]
        let mut output_type = {
            use crate::input::InputKind;
            use std::path::Path;

            // Do not launch the pager if NONE of the input files exist
            let mut paging_mode = self.config.paging_mode;
            if self.config.paging_mode != PagingMode::Never {
                let call_pager = inputs.iter().any(|input| {
                    if let InputKind::OrdinaryFile(ref path) = input.kind {
                        Path::new(path).exists()
                    } else {
                        true
                    }
                });
                if !call_pager {
                    paging_mode = PagingMode::Never;
                }
            }

            let wrapping_mode = self.config.wrapping_mode;

            OutputType::from_mode(paging_mode, wrapping_mode, self.config.pager)?
        };

        #[cfg(not(feature = "paging"))]
        let mut output_type = OutputType::stdout();

        let attached_to_pager = output_type.is_pager();
        let stdout_identifier = if cfg!(windows) || attached_to_pager {
            None
        } else {
            clircle::Identifier::stdout()
        };

        let (writer, has_output_buf): (&'_ mut dyn Write, bool) =
            if let Some(output_buffer) = output_buffer {
                (output_buffer, true)
            } else {
                (output_type.handle()?, false)
            };
        let mut no_errors: bool = true;
        let stderr = io::stderr();

        for (index, input) in inputs.into_iter().enumerate() {
            let identifier = stdout_identifier.as_ref();
            let is_first = index == 0;
            let result = self.print_input(input, writer, identifier, is_first);
            if let Err(error) = result {
                if has_output_buf {
                    // It doesn't make much sense to send errors straight to stderr if the user
                    // provided their own buffer, so we just return it.
                    return Err(error);
                } else {
                    if attached_to_pager {
                        handle_error(&error, writer);
                    } else {
                        handle_error(&error, &mut stderr.lock());
                    }
                }
                no_errors = false;
            }
        }

        Ok(no_errors)
    }

    fn print_input(
        &self,
        input: Input,
        writer: OutputHandle,
        stdout_identifier: Option<&Identifier>,
        is_first: bool,
    ) -> Result<()> {
        let mut opened_input = input.open(
            stdout_identifier,
            #[cfg(feature = "lessopen")]
            self.config.use_lessopen,
        )?;

        if self.config.loop_through {
            let mut printer = SimplePrinter::new(self.config);
            self.print_file(
                &mut printer,
                writer,
                &mut opened_input,
                !is_first,
            )
        } else {
            let mut printer = InteractivePrinter::new(
                self.config,
                self.assets,
                &mut opened_input,
            )?;
            self.print_file(
                &mut printer,
                writer,
                &mut opened_input,
                !is_first,
            )
        }
    }

    fn print_file(
        &self,
        printer: &mut impl Printer,
        writer: OutputHandle,
        input: &mut OpenedInput,
        add_header_padding: bool,
    ) -> Result<()> {
        if input.reader.content_type.is_some() || self.config.style_components.header() {
            printer.print_header(writer, input, add_header_padding)?;
        }

        if input.reader.content_type.is_some() {
            let line_ranges = self.config.visible_lines.0.clone();

            self.print_file_ranges(printer, writer, &mut input.reader, &line_ranges)?;
        }
        printer.print_footer(writer, input)?;

        Ok(())
    }

    fn print_file_ranges(
        &self,
        printer: &mut impl Printer,
        writer: OutputHandle,
        reader: &mut InputReader,
        line_ranges: &LineRanges,
    ) -> Result<()> {
        let mut line_buffer = Vec::new();
        let mut line_number: usize = 1;

        let mut first_range: bool = true;
        let mut mid_range: bool = false;

        let style_snip = self.config.style_components.snip();

        while reader.read_line(&mut line_buffer)? {
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
                RangeCheckResult::AfterLastRange => {
                    break;
                }
            }

            line_number += 1;
            line_buffer.clear();
        }
        Ok(())
    }
}
