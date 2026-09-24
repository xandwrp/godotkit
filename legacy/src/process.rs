use std::{
    io::{self, BufRead, BufReader, Read},
    process::{Command, ExitStatus, Output, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OutputStream {
    Stdout,
    Stderr,
}

#[derive(Clone, Debug)]
pub(crate) struct OutputLine {
    pub(crate) sequence: usize,
    pub(crate) stream: OutputStream,
    pub(crate) bytes: Vec<u8>,
    pub(crate) observed_at_unix_ms: u64,
}

#[derive(Debug)]
pub(crate) struct CapturedOutput {
    pub(crate) output: Output,
    pub(crate) lines: Vec<OutputLine>,
    pub(crate) pid: u32,
    pub(crate) timed_out: bool,
}

impl CapturedOutput {
    #[cfg(test)]
    pub(crate) fn from_output(output: Output) -> Self {
        let mut lines = Vec::new();
        collect_existing_lines(&mut lines, OutputStream::Stdout, &output.stdout);
        collect_existing_lines(&mut lines, OutputStream::Stderr, &output.stderr);
        Self {
            output,
            lines,
            pid: 0,
            timed_out: false,
        }
    }

    pub(crate) fn retaining_lines(&self, retain: impl Fn(usize) -> bool) -> Self {
        let lines: Vec<_> = self
            .lines
            .iter()
            .enumerate()
            .filter_map(|(index, line)| retain(index).then_some(line.clone()))
            .collect();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        for line in &lines {
            match line.stream {
                OutputStream::Stdout => stdout.extend_from_slice(&line.bytes),
                OutputStream::Stderr => stderr.extend_from_slice(&line.bytes),
            }
        }
        Self {
            output: Output {
                status: self.output.status,
                stdout,
                stderr,
            },
            lines,
            pid: self.pid,
            timed_out: self.timed_out,
        }
    }
}

fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
fn collect_existing_lines(lines: &mut Vec<OutputLine>, stream: OutputStream, bytes: &[u8]) {
    let mut reader = BufReader::new(bytes);
    loop {
        let mut line = Vec::new();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) => break,
            Ok(_) => lines.push(OutputLine {
                sequence: lines.len(),
                stream,
                bytes: line,
                observed_at_unix_ms: timestamp(),
            }),
            Err(_) => unreachable!(),
        }
    }
}

fn read_lines(
    reader: impl Read,
    stream: OutputStream,
    sender: mpsc::Sender<OutputLine>,
) -> io::Result<()> {
    let mut reader = BufReader::new(reader);
    loop {
        let mut bytes = Vec::new();
        if reader.read_until(b'\n', &mut bytes)? == 0 {
            return Ok(());
        }
        if sender
            .send(OutputLine {
                sequence: 0,
                stream,
                bytes,
                observed_at_unix_ms: timestamp(),
            })
            .is_err()
        {
            return Ok(());
        }
    }
}

fn terminate(child: &mut std::process::Child) -> io::Result<ExitStatus> {
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .output();
    }
    let _ = child.kill();
    child.wait()
}

pub(crate) fn run(command: &mut Command, timeout: Option<Duration>) -> io::Result<CapturedOutput> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let pid = child.id();
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("child stdout pipe was unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("child stderr pipe was unavailable"))?;
    let (sender, receiver) = mpsc::channel();
    let stdout_sender = sender.clone();
    let stdout_reader =
        thread::spawn(move || read_lines(stdout, OutputStream::Stdout, stdout_sender));
    let stderr_reader = thread::spawn(move || read_lines(stderr, OutputStream::Stderr, sender));
    let began = Instant::now();
    let (status, timed_out) = if let Some(timeout) = timeout {
        loop {
            if let Some(status) = child.try_wait()? {
                break (status, false);
            }
            if began.elapsed() >= timeout {
                break (terminate(&mut child)?, true);
            }
            thread::sleep(Duration::from_millis(20));
        }
    } else {
        (child.wait()?, false)
    };
    stdout_reader
        .join()
        .map_err(|_| io::Error::other("stdout capture thread panicked"))??;
    stderr_reader
        .join()
        .map_err(|_| io::Error::other("stderr capture thread panicked"))??;
    let mut lines: Vec<_> = receiver.into_iter().collect();
    for (sequence, line) in lines.iter_mut().enumerate() {
        line.sequence = sequence;
    }
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    for line in &lines {
        match line.stream {
            OutputStream::Stdout => stdout.extend_from_slice(&line.bytes),
            OutputStream::Stderr => stderr.extend_from_slice(&line.bytes),
        }
    }
    Ok(CapturedOutput {
        output: Output {
            status,
            stdout,
            stderr,
        },
        lines,
        pid,
        timed_out,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn interleaved_command() -> Command {
        if cfg!(windows) {
            let mut command = Command::new("powershell");
            command.args([
                "-NoProfile",
                "-Command",
                "[Console]::Out.WriteLine('out one'); Start-Sleep -Milliseconds 80; [Console]::Error.WriteLine('err one'); Start-Sleep -Milliseconds 80; [Console]::Out.Write('out two')",
            ]);
            command
        } else {
            let mut command = Command::new("sh");
            command.args([
                "-c",
                "printf 'out one\\n'; sleep 0.08; printf 'err one\\n' >&2; sleep 0.08; printf 'out two'",
            ]);
            command
        }
    }

    #[test]
    fn captures_exact_streams_and_cross_stream_observation_order() {
        let captured = run(&mut interleaved_command(), None).unwrap();
        assert!(captured.output.status.success());
        let expected_stdout = if cfg!(windows) {
            b"out one\r\nout two".as_slice()
        } else {
            b"out one\nout two".as_slice()
        };
        let expected_stderr = if cfg!(windows) {
            b"err one\r\n".as_slice()
        } else {
            b"err one\n".as_slice()
        };
        assert_eq!(captured.output.stdout, expected_stdout);
        assert_eq!(captured.output.stderr, expected_stderr);
        assert!(captured.pid > 0);
        assert!(!captured.timed_out);
        assert_eq!(captured.lines.len(), 3);
        assert_eq!(captured.lines[0].stream, OutputStream::Stdout);
        assert_eq!(
            captured.lines[0].bytes,
            if cfg!(windows) {
                b"out one\r\n".as_slice()
            } else {
                b"out one\n".as_slice()
            }
        );
        assert_eq!(captured.lines[1].stream, OutputStream::Stderr);
        assert_eq!(captured.lines[1].bytes, expected_stderr);
        assert_eq!(captured.lines[2].stream, OutputStream::Stdout);
        assert_eq!(captured.lines[2].bytes, b"out two");
        assert!(
            captured
                .lines
                .windows(2)
                .all(|pair| pair[0].observed_at_unix_ms <= pair[1].observed_at_unix_ms)
        );
    }

    #[test]
    fn retained_lines_rebuild_each_stream_without_changing_bytes() {
        let output = Output {
            status: Default::default(),
            stdout: b"out one\nout two\n".to_vec(),
            stderr: b"err one\n".to_vec(),
        };
        let captured = CapturedOutput::from_output(output).retaining_lines(|index| index != 1);
        assert_eq!(captured.output.stdout, b"out one\n");
        assert_eq!(captured.output.stderr, b"err one\n");
    }
}
