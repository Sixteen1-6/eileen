//! Tests of the `ipc` module's pipes. The Windows tests make real pipes, named after this
//! process so two runs never share one; the client of
//! `a_connection_names_the_process_at_each_end` is this test binary run again as a helper.
//! Every wait has a bound, and nothing sleeps.

use crate::ipc::{IpcError, MAX_PIPE_NAME, pipe_path};

#[test]
fn a_pipe_name_is_one_plain_path_component() {
    assert_eq!(
        pipe_path("eileen-speech-sess_01JX").unwrap(),
        r"\\.\pipe\eileen-speech-sess_01JX",
        "a plain name becomes the pipe's path"
    );
    let longest = "a".repeat(MAX_PIPE_NAME);
    assert!(pipe_path(&longest).is_ok(), "the longest name is taken");
    let too_long = "a".repeat(MAX_PIPE_NAME + 1);
    for bad in [
        "",
        r"a\b",
        "a/b",
        "a b",
        "a*",
        "a?",
        "a:b",
        ".",
        "..",
        "a.",
        "a.b",
        "caf\u{e9}",
        "a\0",
        &too_long,
    ] {
        assert!(
            matches!(pipe_path(bad), Err(IpcError::BadName)),
            "{bad:?} could name something other than one pipe"
        );
    }
}

#[cfg(not(windows))]
#[test]
fn every_call_answers_unsupported_off_windows() {
    use crate::ipc::{PipeConn, PipeServer};

    assert!(
        matches!(PipeServer::bind("eileen-test"), Err(IpcError::Unsupported)),
        "no pipe is served where callers cannot be checked"
    );
    assert!(
        matches!(PipeConn::connect("eileen-test"), Err(IpcError::Unsupported)),
        "and none is connected to"
    );
}

#[cfg(windows)]
pub(super) mod win {
    use std::os::windows::io::AsHandle as _;
    use std::path::Path;

    use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _, BufReader};
    use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
    use tokio::time::timeout;

    use super::super::{EXITS_WITHIN, TempDir, current_thread, helper};
    use crate::ipc::{IpcError, Moment, PipeConn, PipeServer, pipe_path};
    use crate::spawn;

    /// The helper's variable for the pipe it connects to.
    const PIPE_ENV: &str = "EILEEN_SV_PIPE";

    /// A pipe name no other test and no other run of the tests uses.
    fn unique(label: &str) -> String {
        format!("eileen-test-{}-{label}", std::process::id())
    }

    /// Awaits `future`, failing the test when it takes longer than any pipe call should.
    async fn soon<T>(what: &str, future: impl Future<Output = T>) -> T {
        timeout(EXITS_WITHIN, future).await.unwrap_or_else(|_| panic!("{what} did not finish"))
    }

    /// An instance of the pipe `name` made the plain way, as any process of the user can:
    /// no first-instance flag and the default DACL.
    fn plain_instance(name: &str, max_instances: usize) -> std::io::Result<NamedPipeServer> {
        ServerOptions::new().max_instances(max_instances).create(pipe_path(name).unwrap())
    }

    /// Sends `text` from `from` and reads it at `to`.
    async fn carries(from: &mut PipeConn, to: &mut PipeConn, text: &str) {
        from.write_all(text.as_bytes()).await.unwrap();
        let mut got = vec![0_u8; text.len()];
        soon("the read", to.read_exact(&mut got)).await.unwrap();
        assert_eq!(got, text.as_bytes(), "the bytes arrive unchanged");
    }

    /// The helper child: connects to the pipe its environment names, writes the process ids
    /// it reads from its end, and stays until the server closes the connection.
    pub(in crate::tests) fn helper_client() {
        let name = std::env::var(PIPE_ENV).unwrap();
        current_thread().block_on(async {
            let mut conn = PipeConn::connect(&name).unwrap();
            let line = format!("{} {}\n", conn.server_pid().unwrap(), conn.client_pid().unwrap());
            conn.write_all(line.as_bytes()).await.unwrap();
            let mut rest = Vec::new();
            let _closed = soon("the server's close", conn.read_to_end(&mut rest)).await;
        });
    }

    fn helper_spec(dir: &Path, pipe: &str) -> crate::ChildSpec {
        helper("pipe-client", dir).envs([(PIPE_ENV, pipe)])
    }

    #[test]
    fn bind_refuses_a_name_that_exists() {
        current_thread().block_on(async {
            let squatted = unique("squatted");
            let _squatter = plain_instance(&squatted, 4).unwrap();
            assert!(
                matches!(PipeServer::bind(&squatted), Err(IpcError::NameTaken { .. })),
                "another process's pipe holds the name"
            );

            let ours = unique("bound-twice");
            let _first = PipeServer::bind(&ours).unwrap();
            assert!(
                matches!(PipeServer::bind(&ours), Err(IpcError::NameTaken { .. })),
                "and so does our own first instance"
            );
        });
    }

    #[test]
    fn a_connection_names_the_process_at_each_end() {
        let dir = TempDir::new("pipe-pids");
        current_thread().block_on(async {
            let name = unique("pids");
            let mut server = PipeServer::bind(&name).unwrap();
            let mut child = spawn(&helper_spec(&dir.0, &name)).unwrap();
            let child_pid = child.id().unwrap();
            let me = std::process::id();

            let conn = soon("accept", server.accept()).await.unwrap();
            assert_eq!(conn.client_pid().unwrap(), child_pid, "the client end is the child");
            assert_eq!(conn.server_pid().unwrap(), me, "the server end is this process");

            let mut lines = BufReader::new(conn).lines();
            let line = soon("the child's line", lines.next_line()).await.unwrap().unwrap();
            assert_eq!(line, format!("{me} {child_pid}"), "the client reads the same two ids");

            drop(lines);
            let status = soon("the child's exit", child.wait()).await.unwrap();
            assert!(status.success(), "the helper saw the close and left: {status:?}");
        });
    }

    #[test]
    fn an_instance_listens_while_connections_are_open() {
        current_thread().block_on(async {
            let name = unique("listens");
            let mut server = PipeServer::bind(&name).unwrap();
            let before = Moment::now();

            let mut first_client = PipeConn::connect(&name).unwrap();
            let mut first_served = soon("the first accept", server.accept()).await.unwrap();
            // The first connection is still open, and the next client finds an instance.
            let mut second_client = PipeConn::connect(&name).unwrap();
            let mut second_served = soon("the second accept", server.accept()).await.unwrap();
            let after = Moment::now();

            carries(&mut first_client, &mut first_served, "one, to the server").await;
            carries(&mut first_served, &mut first_client, "one, to the client").await;
            carries(&mut second_client, &mut second_served, "two, to the server").await;
            carries(&mut second_served, &mut second_client, "two, to the client").await;

            assert!(before <= first_client.since(), "connected after the test began");
            assert!(first_client.since() <= first_served.since(), "accepted after it connected");
            assert!(first_served.since() <= second_served.since(), "the second was accepted later");
            assert!(second_served.since() <= after, "and before the test read the clock again");
            assert!(before.as_filetime() > 0, "a file time, not zero");
        });
    }

    #[test]
    fn a_next_instance_that_cannot_be_made_is_an_error_and_keeps_the_connection() {
        current_thread().block_on(async {
            let name = unique("full");
            let mut server = PipeServer::bind_limited(&name, 2).unwrap();
            let mut client = PipeConn::connect(&name).unwrap();
            // Another process of the user takes the pipe's last instance, as a worker could.
            let squatter = plain_instance(&name, 2).unwrap();

            for attempt in 1..=2 {
                let refused = soon("a refused accept", server.accept()).await;
                assert!(
                    matches!(&refused, Err(IpcError::NextInstance { name: n, .. }) if *n == name),
                    "attempt {attempt} says the next instance could not be made: {refused:?}"
                );
            }

            drop(squatter);
            let mut accepted = soon("the accept that follows", server.accept()).await.unwrap();
            assert_eq!(
                accepted.client_pid().unwrap(),
                std::process::id(),
                "the client that waited is handed over"
            );
            assert!(client.since() <= accepted.since(), "stamped when it connected");
            carries(&mut client, &mut accepted, "kept").await;

            // And the instance made in its place serves the next client. The pipe is full
            // again until the runtime has closed the first connection's handle, so the server
            // is called again, as its caller does, until the instance can be made.
            drop(accepted);
            drop(client);
            let mut second_client = PipeConn::connect(&name).unwrap();
            let retried = async {
                loop {
                    match server.accept().await {
                        Err(IpcError::NextInstance { .. }) => tokio::task::yield_now().await,
                        other => return other,
                    }
                }
            };
            let mut next_served = soon("the next accept", retried).await.unwrap();
            carries(&mut second_client, &mut next_served, "next").await;
        });
    }

    #[test]
    fn connect_tells_a_missing_pipe_from_a_busy_one() {
        current_thread().block_on(async {
            let missing = unique("missing");
            assert!(
                matches!(PipeConn::connect(&missing), Err(IpcError::NoServer { .. })),
                "nothing made this pipe"
            );

            let name = unique("busy");
            let _server = PipeServer::bind_limited(&name, 1).unwrap();
            let _client = PipeConn::connect(&name).unwrap();
            assert!(
                matches!(PipeConn::connect(&name), Err(IpcError::Busy { .. })),
                "its one instance holds a connection"
            );
        });
    }

    #[test]
    fn every_instance_grants_only_the_current_user() {
        current_thread().block_on(async {
            let name = unique("dacl");
            let mut server = PipeServer::bind(&name).unwrap();
            assert!(server.grants_only_user().unwrap(), "the first instance");

            let _client = PipeConn::connect(&name).unwrap();
            let _conn = soon("accept", server.accept()).await.unwrap();
            assert!(server.grants_only_user().unwrap(), "the instance made after it");

            // The read-back tells the difference: a pipe made the plain way takes the
            // default DACL, which also grants the system, administrators and everyone.
            let plain = plain_instance(&unique("dacl-plain"), 4).unwrap();
            assert!(
                !crate::ffi::pipe::grants_only_user(plain.as_handle()).unwrap(),
                "a default DACL grants more than the user"
            );
        });
    }

    #[test]
    fn a_client_that_left_before_the_accept_does_not_stop_the_server() {
        current_thread().block_on(async {
            let name = unique("left");
            let mut server = PipeServer::bind(&name).unwrap();
            // A plain file handle closes at once; a tokio pipe closes only when the runtime
            // next runs, which would leave the client connected.
            let mut plain = std::fs::OpenOptions::new();
            plain.read(true).write(true);
            drop(plain.open(pipe_path(&name).unwrap()).unwrap());
            // The OS may report the client that is gone as an error or as a connection, and
            // such a connection carries nothing.
            let gone = soon("the accept of the client that left", server.accept()).await;
            if let Ok(mut gone) = gone {
                let mut byte = [0_u8; 1];
                let read = soon("the read of a closed pipe", gone.read(&mut byte)).await;
                assert!(matches!(read, Ok(0) | Err(_)), "nothing arrives from it: {read:?}");
            }

            let mut client = PipeConn::connect(&name).unwrap();
            let mut accepted = soon("the accept after it", server.accept()).await.unwrap();
            carries(&mut client, &mut accepted, "still serving").await;
        });
    }
}
