use std::mem;

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::token::Comma;
use syn::{
    parse_macro_input, parse_quote, parse_quote_spanned, parse_str, Error, Expr, Ident, Lit,
};

#[derive(Debug, Clone)]
enum FmtArg {
    String(String),
    Display(Expr),
    Format(String, Option<Expr>),
}

fn write_impl(tokens: proc_macro::TokenStream, ln: bool) -> proc_macro::TokenStream {
    let args = parse_macro_input!(tokens with Punctuated::<Expr, Comma>::parse_terminated);
    let mut args = args.into_iter();

    let Some(writer) = args.next() else {
        return Error::new(Span::call_site(), "expected arguments")
            .to_compile_error()
            .into();
    };

    let fmt_str = match args.next() {
        Some(Expr::Lit(e)) => match e.lit {
            Lit::Str(s) => s,
            l => {
                return Error::new(l.span(), "expected literal string")
                    .to_compile_error()
                    .into()
            }
        },
        Some(e) => {
            return Error::new(e.span(), "expected literal string")
                .to_compile_error()
                .into()
        }
        None => parse_quote! { "" },
    };
    let sspan = fmt_str.span();
    let invalid_fmt_str = Error::new(sspan, "invalid format string")
        .to_compile_error()
        .into();
    let mismatch_args = Error::new(
        sspan,
        "the number of arguments does not match with the format string",
    )
    .to_compile_error()
    .into();
    let fmt_str = fmt_str.value();

    let mut iter = fmt_str.chars().peekable();
    let mut in_brace = false;
    let mut current_string = String::new();
    let mut fmt_args = Vec::new();
    while let Some(c) = iter.next() {
        if c == '{' {
            if iter.next_if_eq(&'{').is_none() {
                if !in_brace {
                    if !current_string.is_empty() {
                        fmt_args.push(FmtArg::String(mem::take(&mut current_string)));
                    }
                    in_brace = true;
                    continue;
                } else {
                    return invalid_fmt_str;
                }
            }
        } else if c == '}' {
            if in_brace {
                let pat = mem::take(&mut current_string);
                if pat.is_empty() {
                    fmt_args.push(FmtArg::Display(match args.next() {
                        Some(e) => e,
                        None => return mismatch_args,
                    }))
                } else if let Ok(ident) = parse_str::<Ident>(&pat) {
                    let e: Expr = parse_quote_spanned! { sspan => #ident };
                    fmt_args.push(FmtArg::Display(e));
                } else if pat.starts_with(':') {
                    fmt_args.push(FmtArg::Format(
                        pat,
                        match args.next() {
                            e @ Some(_) => e,
                            None => return mismatch_args,
                        },
                    ))
                } else {
                    fmt_args.push(FmtArg::Format(pat, None))
                }
                in_brace = false;
                continue;
            } else {
                if iter.next_if_eq(&'}').is_none() {
                    return invalid_fmt_str;
                }
            }
        }
        current_string.push(c);
    }
    if !current_string.is_empty() {
        fmt_args.push(FmtArg::String(current_string));
    }
    if args.next().is_some() {
        return mismatch_args;
    }

    let arg_names: Vec<_> = fmt_args
        .iter()
        .enumerate()
        .map(|(i, _)| format_ident!("_{}", i))
        .collect();

    let mut body: TokenStream = arg_names
        .iter()
        .cloned()
        .zip(fmt_args.iter())
        .map(|(a, fmt_arg)| match fmt_arg {
            FmtArg::String(_) => quote! { self.write_str(#a)?; },
            FmtArg::Display(_) => quote! { self.write_str(#a)?; },
            FmtArg::Format(_, _) => quote! { self.write_fmt(#a)?; },
        })
        .collect();

    let fn_args: TokenStream = arg_names
        .iter()
        .zip(fmt_args.iter())
        .map(|(a, fmt_arg)| match fmt_arg {
            FmtArg::String(_) | FmtArg::Display(_) => quote! { #a: &str, },
            FmtArg::Format(_, _) => quote! { #a: std::fmt::Arguments, },
        })
        .collect();

    let relay: TokenStream = arg_names.into_iter().map(|a| quote! { #a, }).collect();

    let call: TokenStream = fmt_args
        .into_iter()
        .map(|fmt_arg| match fmt_arg {
            FmtArg::String(s) => quote! { #s, },
            FmtArg::Display(e) => quote! { (#e).to_compact_string().as_str(), },
            FmtArg::Format(mut f, e) => {
                f = format!("{{{}}}", f);
                match e {
                    Some(e) => quote! { std::format_args!(#f, (#e)), },
                    None => quote! { std::format_args!(#f), },
                }
            }
        })
        .collect();

    if ln {
        body.extend(quote! {
            self.write_char('\n')?;
        })
    }

    let whole = quote! {
        {
            use std::fmt::Write as _;
            use compact_str::ToCompactString as _;
            trait __Helper1 {
                fn __run(&mut self, #fn_args) -> std::fmt::Result;
            }
            impl<W: std::fmt::Write + ?std::marker::Sized> __Helper1 for W {
                fn __run(&mut self, #fn_args) -> std::fmt::Result {
                    #body
                    Ok(())
                }
            }
            trait __Helper2 {
                fn __run(&mut self, #fn_args) -> std::io::Result<()>;
            }
            impl<W: std::io::Write + ?std::marker::Sized> __Helper2 for W {
                fn __run(&mut self, #fn_args) -> std::io::Result<()> {
                    struct __FmtWriter<'a, W: std::io::Write + ?std::marker::Sized>(&'a mut W, std::option::Option<std::io::Error>);
                    impl<'a, W: std::io::Write + ?std::marker::Sized> std::fmt::Write for __FmtWriter<'a, W> {
                        fn write_str(&mut self, s: &str) -> std::fmt::Result {
                            if let Err(e) = self.0.write_all(s.as_bytes()) {
                                self.1 = Some(e);
                                Err(std::fmt::Error)
                            } else {
                                Ok(())
                            }
                        }
                    }
                    let mut __w = __FmtWriter(self, None);
                    if __w.__run(#relay).is_err() {
                        Err(__w.1.unwrap())
                    } else {
                        Ok(())
                    }
                }
            }
            (#writer).__run(#call)
        }
    };

    whole.into()
}

#[proc_macro]
pub fn write(tokens: proc_macro::TokenStream) -> proc_macro::TokenStream {
    write_impl(tokens, false)
}

#[proc_macro]
pub fn writeln(tokens: proc_macro::TokenStream) -> proc_macro::TokenStream {
    write_impl(tokens, true)
}
