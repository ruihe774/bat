use std::cmp::Ordering;
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
    "matlab",
    "mm",
    "ml",
    "pas",
    "pm",
    "php",
    "ps1",
    "prolog",
    "py",
    "r",
    "rb",
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

pub(crate) fn guesslang(mut t: String) -> Option<&'static str> {
    let environment = ENVIRONMENT
        .get_or_init(|| Environment::default().into_arc());
    let session = SESSION
        .get_or_try_init(|| {
            SessionBuilder::new(environment)?
                .with_custom_op_lib(env!("OCOS_LIB_PATH"))? // path to onnxruntime extensions "libortextensions"
                .with_optimization_level(ort::GraphOptimizationLevel::Disable)? // the model is already optimized for CPU
                .with_model_from_memory(include_bytes!("../assets/guesslang.onnx"))
        })
        .expect("failed to init guesslang session");

    t.truncate(10000); // this is maximum of model input
    let input = CowArray::from(Array0::from_elem((), t)).into_dyn();
    let inputs = vec![Value::from_array(session.allocator(), &input).ok()?]; // may fail if string contains \0
    let outputs = session.run(inputs).ok()?; // the model may error with very short input
    let output: OrtOwnedTensor<f32, _> = outputs[0].try_extract().ok()?; // WTH is going on?
    let output = output.view();
    let (index, prob) = output
        .iter()
        .cloned()
        .enumerate()
        .max_by(|(_, l), (_, r)| l.partial_cmp(r).unwrap_or(Ordering::Equal))
        .unwrap();
    let lang = LABELS[index];
    (prob > 0.5).then_some(lang)
}
