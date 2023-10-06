use std::sync::Arc;

use ndarray::{Array0, CowArray};
use once_cell::sync::OnceCell;
use ort::{tensor::OrtOwnedTensor, Environment, InMemorySession, SessionBuilder, Value};

const LABELS: [&str; 54] = [
    "asm",
    "bat",
    "c",
    "cs",
    "cpp",
    "clj",
    "cmake",
    "cbl",
    "coffee",
    "css",
    "csv",
    "dart",
    "dm",
    "dockerfile",
    "ex",
    "erl",
    "f90",
    "go",
    "groovy",
    "hs",
    "html",
    "ini",
    "java",
    "js",
    "json",
    "jl",
    "kt",
    "lisp",
    "lua",
    "makefile",
    "md",
    "matla",
    "mm",
    "ml",
    "pas",
    "pm",
    "php",
    "ps1",
    "prolog",
    "py",
    "r",
    "r",
    "rs",
    "scala",
    "sh",
    "sql",
    "swift",
    "tex",
    "toml",
    "ts",
    "v",
    "vba",
    "xml",
    "yaml",
];

static ENVIRONMENT: OnceCell<Arc<Environment>> = OnceCell::new();
static SESSION: OnceCell<InMemorySession> = OnceCell::new();

pub(crate) fn guesslang(t: String) -> Option<&'static str> {
    let environment = ENVIRONMENT.get_or_init(|| Environment::default().into_arc());
    let session = SESSION
        .get_or_try_init(|| {
            SessionBuilder::new(environment)?
                .with_custom_op_lib(env!("OCOS_LIB_PATH"))? // path to onnxruntime extensions "libortextensions"
                .with_model_from_memory(include_bytes!("../assets/guesslang.onnx"))
        })
        .expect("failed to init guesslang session");

    let input = CowArray::from(Array0::from_elem((), t)).into_dyn();
    let inputs = vec![Value::from_array(session.allocator(), &input)
        .expect("failed to alloc guesslang model input")];
    let outputs = match session.run(inputs) {
        Ok(r) => r,
        Err(_) => return None,
    };
    let output: OrtOwnedTensor<i64, _> = outputs[0]
        .try_extract()
        .expect("failed to extract guesslang output");
    let output = output.view();
    let lang = LABELS[*output.first().unwrap() as usize];
    return Some(lang);
}
