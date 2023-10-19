use std::cell::Cell;
use std::sync::Arc;
use std::{cmp::Ordering, fmt::Debug};

use ndarray::{Array0, CowArray};
use once_cell::unsync::OnceCell;
use ort::{tensor::OrtOwnedTensor, Environment, OwnedInMemorySession, SessionBuilder, Value};

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

pub(crate) struct GuessLang {
    environment: OnceCell<Arc<Environment>>,
    session: OnceCell<OwnedInMemorySession>,
    model: Cell<Vec<u8>>,
}

impl GuessLang {
    pub fn new(model: Vec<u8>) -> GuessLang {
        GuessLang {
            environment: OnceCell::new(),
            session: OnceCell::new(),
            model: Cell::new(model),
        }
    }

    pub fn guess(&self, mut t: String) -> Option<&'static str> {
        let environment = self
            .environment
            .get_or_init(|| Environment::default().into_arc());
        let session = self
            .session
            .get_or_try_init(|| {
                SessionBuilder::new(environment)?
                    .with_enable_ort_custom_ops()?
                    .with_model_from_owned_memory(self.model.take())
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
}

impl Debug for GuessLang {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GuessLang {}")
    }
}
