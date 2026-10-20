use std::cell::RefCell;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

struct ReferenceParser {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl ReferenceParser {
    fn new() -> Self {
        let parser = std::env::var_os("RYL_REFERENCE_PARSER")
            .expect("RYL_REFERENCE_PARSER points to the yaml-parser CLI");
        let mut child = Command::new("node")
            .args(["-e", include_str!("reference.cjs")])
            .arg(parser)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("start the YAML reference parser");
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            input,
            output,
        }
    }

    fn value(&mut self, input: &str) -> String {
        let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
        writeln!(
            self.input,
            "{}",
            serde_json::to_string(&normalized).unwrap()
        )
        .unwrap();
        let mut value = String::new();
        assert_ne!(
            self.output.read_line(&mut value).unwrap(),
            0,
            "reference parser exited"
        );
        serde_json::from_str(&value).expect("reference scalar value")
    }
}

impl Drop for ReferenceParser {
    fn drop(&mut self) {
        self.child.kill().expect("stop reference parser");
        self.child.wait().expect("reap reference parser");
    }
}

pub fn block_value(input: &str) -> String {
    thread_local! {
        static PARSER: RefCell<ReferenceParser> = RefCell::new(ReferenceParser::new());
    }
    PARSER.with(|parser| parser.borrow_mut().value(input))
}
