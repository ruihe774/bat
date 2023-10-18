use std::cell::RefCell;
use std::sync::Arc;
use std::{cmp::Ordering, fmt::Debug};

use ndarray::{Array0, CowArray};
use once_cell::sync::OnceCell;
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

#[derive(Debug)]
pub(crate) struct GuessLang {
    environment: OnceCell<Arc<Environment>>,
    session: OnceCell<OwnedInMemorySession>,
    model: RefCell<Option<Vec<u8>>>,
}

impl GuessLang {
    pub fn new(model: Vec<u8>) -> GuessLang {
        GuessLang {
            environment: OnceCell::new(),
            session: OnceCell::new(),
            model: RefCell::new(Some(model)),
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
                    .with_model_from_owned_memory(self.model.take().unwrap())
            })
            .expect("failed to init guesslang session");

        t.truncate(10000);  // this is maximum of model input
        let input = CowArray::from(Array0::from_elem((), t)).into_dyn();
        let inputs = vec![Value::from_array(session.allocator(), &input)
            .expect("failed to alloc guesslang model input")];
        let outputs = match session.run(inputs) {
            Ok(r) => r,
            Err(_) => return None, // the model may error with very short input
        };
        let output: OrtOwnedTensor<f32, _> = outputs[0]
            .try_extract()
            .expect("failed to extract guesslang output");
        let output = output.view();
        let (index, prob) = output
            .iter()
            .cloned()
            .enumerate()
            .max_by(|(_, l), (_, r)| l.partial_cmp(r).unwrap_or(Ordering::Equal))
            .unwrap();
        let lang = LABELS[index];
        if prob > 0.5 {
            Some(lang)
        } else {
            None
        }
    }
}
