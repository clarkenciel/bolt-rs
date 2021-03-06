use std::collections::HashMap;

use bolt_client_macros::*;
use bolt_proto::message::*;
use bolt_proto::{Message, Value};

use crate::error::*;
use crate::Client;

impl Client {
    /// Send an `INIT` message to the server.
    ///
    /// # Description
    /// The `INIT` message is a Bolt v1 client message used once to initialize the session. This message is always the
    /// first message the client sends after negotiating protocol version via the initial handshake. Sending any message
    /// other than `INIT` as the first message to the server will result in a `FAILURE`. The client must acknowledge
    /// failures using `ACK_FAILURE`, after which `INIT` may be reattempted.
    ///
    /// # Response
    /// - `SUCCESS {…}` if initialization has completed successfully
    /// - `FAILURE {"code": …​, "message": …​}` if the request was malformed, or if initialization
    ///     cannot be performed at this time, or if the authorization failed.
    #[bolt_version(1, 2)]
    pub async fn init(
        &mut self,
        client_name: String,
        auth_token: HashMap<String, impl Into<Value>>,
    ) -> Result<Message> {
        let init_msg = Init::new(
            client_name,
            auth_token.into_iter().map(|(k, v)| (k, v.into())).collect(),
        );
        self.send_message(Message::Init(init_msg)).await?;
        self.read_message().await
    }

    /// Send a `RUN` message to the server.
    ///
    /// # Description
    /// The `RUN` message is a client message used to pass a statement for execution on the server.
    /// On receipt of a `RUN` message, the server will start a new job by executing the statement with the parameters
    /// (optionally) supplied. If successful, the subsequent response will consist of a single `SUCCESS` message; if
    /// not, a `FAILURE` response will be sent instead. A successful job will always produce a result stream which must
    /// then be explicitly consumed (via `PULL_ALL` or `DISCARD_ALL`), even if empty.
    ///
    /// Depending on the statement you are executing, additional metadata may be returned in both the `SUCCESS` message
    /// from the `RUN`, as well as in the final `SUCCESS` after the stream has been consumed. It is up to the statement
    /// you are running to determine what meta data to return. Notably, most queries will contain a `fields` metadata
    /// section in the `SUCCESS` message for the RUN statement, which lists the result record field names, and a
    /// `result_available_after` section measuring the number of milliseconds it took for the results to be available
    /// for consumption.
    ///
    /// In the case where a previous result stream has not yet been fully consumed, an attempt to `RUN` a new job will
    /// trigger a `FAILURE` response.
    ///
    /// If an unacknowledged failure is pending from a previous exchange, the server will immediately respond with a
    /// single `IGNORED` message and take no further action.
    ///
    /// # Response
    /// - `SUCCESS {…}` if the statement has been accepted for execution
    /// - `FAILURE {"code": …​, "message": …​}` if the request was malformed or if a statement may not be executed at this
    ///     time
    #[bolt_version(1, 2)]
    pub async fn run(
        &mut self,
        statement: String,
        parameters: Option<HashMap<String, Value>>,
    ) -> Result<Message> {
        let run_msg = Run::new(statement, parameters.unwrap_or_default());
        self.send_message(Message::Run(run_msg)).await?;
        self.read_message().await
    }

    /// Send a `DISCARD_ALL` message to the server.
    ///
    /// # Description
    /// The `DISCARD_ALL` message is a client message used to discard all remaining items from the active result stream.
    ///
    /// On receipt of a `DISCARD_ALL` message, the server will dispose of all remaining items from the active result
    /// stream, close the stream and send a single `SUCCESS` message to the client. If no result stream is currently
    /// active, the server will respond with a single `FAILURE` message.
    ///
    /// If an unacknowledged failure is pending from a previous exchange, the server will immediately respond with a
    /// single `IGNORED` message and take no further action.
    ///
    /// # Response
    /// - `SUCCESS {…}` if the result stream has been successfully discarded
    /// - `FAILURE {"code": …​, "message": …​}` if no result stream is currently available
    #[bolt_version(1, 2, 3)]
    pub async fn discard_all(&mut self) -> Result<Message> {
        self.send_message(Message::DiscardAll).await?;
        self.read_message().await
    }

    /// Send a `PULL_ALL` message to the server. Returns a tuple containing a `Vec` of the records returned from the
    /// server as well as the summary message (`SUCCESS` or `FAILURE`).
    ///
    /// # Description
    /// The `PULL_ALL` message is a client message used to retrieve all remaining items from the active result stream.
    ///
    /// On receipt of a `PULL_ALL` message, the server will send all remaining result data items to the client, each in
    /// a single `RECORD` message. The server will then close the stream and send a single `SUCCESS` message optionally
    /// containing summary information on the data items sent. If an error is encountered, the server must instead send
    /// a `FAILURE` message, discard all remaining data items and close the stream.
    ///
    /// If an unacknowledged failure is pending from a previous exchange, the server will immediately respond with a
    /// single `IGNORED` message and take no further action.
    ///
    /// # Response
    /// - `SUCCESS {…​}` if the result stream has been successfully transferred
    /// - `FAILURE {"code": …​, "message": …​}` if no result stream is currently available or if retrieval fails
    #[bolt_version(1, 2, 3)]
    pub async fn pull_all(&mut self) -> Result<(Message, Vec<Record>)> {
        self.send_message(Message::PullAll).await?;
        let mut records = vec![];
        loop {
            match self.read_message().await? {
                Message::Record(record) => records.push(record),
                other => return Ok((other, records)),
            }
        }
    }

    /// Send an `ACK_FAILURE` message to the server.
    ///
    /// # Description
    /// The `ACK_FAILURE` message is a client message used to acknowledge a failure the server has sent.
    ///
    /// The following actions are performed by `ACK_FAILURE`:
    /// - clear any outstanding `FAILURE` state
    ///
    /// In some cases, it may be preferable to use `RESET` after a failure, to clear the entire state of the connection.
    ///
    /// # Response
    /// - `SUCCESS {}` if the session was successfully reset
    /// - `FAILURE {"code": …​, "message": …​}` if there is no failure waiting to be cleared
    #[bolt_version(1, 2)]
    pub async fn ack_failure(&mut self) -> Result<Message> {
        self.send_message(Message::AckFailure).await?;
        self.read_message().await
    }

    /// Send a `RESET` message to the server.
    ///
    /// # Description
    /// The `RESET` message is a client message used to return the current session to a "clean" state. It will cause the
    /// session to `IGNORE` any message it is currently processing, as well as any message before `RESET` that had not
    /// yet begun processing. This allows `RESET` to abort long-running operations. It also means clients must be
    /// careful about pipelining `RESET`. Only send this if you are not currently waiting for a result from a prior
    /// message, or if you want to explicitly abort any prior message.
    ///
    /// The following actions are performed by `RESET`:
    /// - force any currently processing message to abort with `IGNORED`
    /// - force any pending messages that have not yet started processing to be `IGNORED`
    /// - clear any outstanding `FAILURE` state
    /// - dispose of any outstanding result records
    /// - rollback the current transaction (if any)
    ///
    /// See [`ack_failure`](Client::ack_failure) for sending a message that only clears `FAILURE` state.
    ///
    /// # Response
    /// - `SUCCESS {}` if the session was successfully reset
    /// - `FAILURE {"code": …​, "message": …​}` if a reset is not currently possible
    #[bolt_version(1, 2, 3, 4)]
    pub async fn reset(&mut self) -> Result<Message> {
        self.send_message(Message::Reset).await?;
        self.read_message().await
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::HashMap;
    use std::convert::TryFrom;
    use std::env;
    use std::iter::FromIterator;

    use bolt_proto::message::*;
    use bolt_proto::value::*;

    use crate::skip_if_handshake_failed;

    use super::*;

    pub(crate) async fn new_client(version: u32) -> Result<Client> {
        let mut client = Client::new(
            env::var("BOLT_TEST_ADDR").unwrap(),
            env::var("BOLT_TEST_DOMAIN").ok().as_deref(),
        )
        .await?;
        client.handshake(&[version, 0, 0, 0]).await?;
        Ok(client)
    }

    pub(crate) async fn initialize_client(client: &mut Client, succeed: bool) -> Result<Message> {
        let username = env::var("BOLT_TEST_USERNAME").unwrap();
        let password = if succeed {
            env::var("BOLT_TEST_PASSWORD").unwrap()
        } else {
            "invalid".to_string()
        };

        let version = client.version.unwrap();
        if [1_u32, 2_u32].contains(&version) {
            client
                .init(
                    "bolt-client/X.Y.Z".to_string(),
                    HashMap::from_iter(vec![
                        ("scheme".to_string(), "basic".to_string()),
                        ("principal".to_string(), username),
                        ("credentials".to_string(), password),
                    ]),
                )
                .await
        } else {
            client
                .hello(HashMap::from_iter(vec![
                    ("user_agent".to_string(), "bolt-client/X.Y.Z".to_string()),
                    ("scheme".to_string(), "basic".to_string()),
                    ("principal".to_string(), username),
                    ("credentials".to_string(), password),
                ]))
                .await
        }
    }

    pub(crate) async fn get_initialized_client(version: u32) -> Result<Client> {
        let mut client = new_client(version).await?;
        initialize_client(&mut client, true).await?;
        Ok(client)
    }

    pub(crate) async fn run_invalid_query(client: &mut Client) -> Result<Message> {
        if client.version.unwrap() > 2 {
            client
                .run_with_metadata(
                    "RETURN invalid query oof as n;".to_string(),
                    Some(HashMap::from_iter(vec![(
                        "some_val".to_string(),
                        Value::from(25.5432),
                    )])),
                    Some(HashMap::from_iter(vec![(
                        "some_key".to_string(),
                        Value::from(true),
                    )])),
                )
                .await
        } else {
            client.run("".to_string(), None).await
        }
    }

    pub(crate) async fn run_valid_query(client: &mut Client) -> Result<Message> {
        if client.version.unwrap() > 2 {
            client
                .run_with_metadata(
                    "RETURN $some_val as n;".to_string(),
                    Some(HashMap::from_iter(vec![(
                        "some_val".to_string(),
                        Value::from(25.5432),
                    )])),
                    Some(HashMap::from_iter(vec![(
                        "some_key".to_string(),
                        Value::from(true),
                    )])),
                )
                .await
        } else {
            client.run("RETURN 1 as n;".to_string(), None).await
        }
    }

    #[tokio::test]
    async fn init() {
        let client = new_client(1).await;
        skip_if_handshake_failed!(client);
        let mut client = client.unwrap();
        let response = initialize_client(&mut client, true).await.unwrap();
        assert!(Success::try_from(response).is_ok());
    }

    #[tokio::test]
    async fn init_fail() {
        let client = new_client(1).await;
        skip_if_handshake_failed!(client);
        let mut client = client.unwrap();
        let response = initialize_client(&mut client, false).await.unwrap();
        assert!(Failure::try_from(response).is_ok());

        // See https://github.com/neo4j/neo4j/pull/8050.
        // The current behavior is to simply close the connection on a failed INIT.
        // Messages now fail to send since connection was closed
        let response = initialize_client(&mut client, true).await;
        assert!(match response {
            Err(Error::ProtocolError(bolt_proto::error::Error::IOError(_))) => true,
            _ => false,
        })
    }

    #[tokio::test]
    async fn ack_failure() {
        let client = get_initialized_client(1).await;
        skip_if_handshake_failed!(client);
        let mut client = client.unwrap();
        let response = run_invalid_query(&mut client).await.unwrap();
        assert!(Failure::try_from(response).is_ok());
        let response = client.ack_failure().await.unwrap();
        assert!(Success::try_from(response).is_ok());
        let response = run_valid_query(&mut client).await.unwrap();
        assert!(Success::try_from(response).is_ok());
    }

    #[tokio::test]
    async fn ack_failure_after_ignored() {
        let client = get_initialized_client(1).await;
        skip_if_handshake_failed!(client);
        let mut client = client.unwrap();
        let response = run_invalid_query(&mut client).await.unwrap();
        assert!(Failure::try_from(response).is_ok());
        let response = run_valid_query(&mut client).await.unwrap();
        assert!(match response {
            Message::Ignored => true,
            _ => false,
        });
        let response = client.ack_failure().await.unwrap();
        assert!(Success::try_from(response).is_ok());
        let response = run_valid_query(&mut client).await.unwrap();
        assert!(Success::try_from(response).is_ok());
    }

    #[tokio::test]
    async fn run() {
        let client = get_initialized_client(1).await;
        skip_if_handshake_failed!(client);
        let mut client = client.unwrap();
        let response = run_valid_query(&mut client).await.unwrap();
        assert!(Success::try_from(response).is_ok());
    }

    #[tokio::test]
    async fn run_pipelined() {
        let client = get_initialized_client(1).await;
        skip_if_handshake_failed!(client);
        let mut client = client.unwrap();
        let messages = vec![
            Message::Run(Run::new("MATCH (n {test: 'v1-pipelined'}) DETACH DELETE n;".to_string(), Default::default())),
            Message::PullAll,
            Message::Run(Run::new("CREATE (:Database {name: 'neo4j', born: 2007, test: 'v1-pipelined'});".to_string(), Default::default())),
            Message::PullAll,
            Message::Run(Run::new(
                "MATCH (neo4j:Database {name: 'neo4j', test: 'v1-pipelined'}) CREATE (:Library {name: 'bolt-client', born: 2019, test: 'v1-pipelined'})-[:CLIENT_FOR]->(neo4j);".to_string(),
                Default::default())),
            Message::PullAll,
            Message::Run(Run::new(
                "MATCH (neo4j:Database {name: 'neo4j', test: 'v1-pipelined'}), (bolt_client:Library {name: 'bolt-client', test: 'v1-pipelined'}) RETURN bolt_client.born - neo4j.born;".to_string(),
                Default::default())),
            Message::PullAll,
        ];
        for response in client.pipeline(messages).await.unwrap() {
            assert!(match response {
                Message::Success(_) => true,
                Message::Record(record) => {
                    assert_eq!(
                        Record::try_from(record).unwrap().fields()[0],
                        Value::from(12_i8)
                    );
                    true
                }
                _ => false,
            });
        }
    }

    #[tokio::test]
    async fn run_and_pull() {
        let client = get_initialized_client(1).await;
        skip_if_handshake_failed!(client);
        let mut client = client.unwrap();
        let response = client
            .run("RETURN 3458376 as n;".to_string(), None)
            .await
            .unwrap();
        assert!(Success::try_from(response).is_ok());

        let (response, records) = client.pull_all().await.unwrap();
        assert!(Success::try_from(response).is_ok());
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].fields(), &[Value::from(3_458_376)]);
    }

    #[tokio::test]
    async fn node_and_rel_creation() {
        let client = get_initialized_client(1).await;
        skip_if_handshake_failed!(client);
        let mut client = client.unwrap();
        let statement = "MATCH (n {test: 'v1-node-rel'}) DETACH DELETE n;".to_string();
        client.run(statement, None).await.unwrap();
        client.pull_all().await.unwrap();

        let statement =
            "CREATE (:Client {name: 'bolt-client', test: 'v1-node-rel'})-[:WRITTEN_IN]->(:Language {name: 'Rust', test: 'v1-node-rel'});"
                .to_string();
        client.run(statement, None).await.unwrap();
        client.pull_all().await.unwrap();
        let statement =
            "MATCH (c {test: 'v1-node-rel'})-[r:WRITTEN_IN]->(l) RETURN c, r, l;".to_string();
        client.run(statement, None).await.unwrap();
        let (_response, records) = client.pull_all().await.unwrap();

        let c = Node::try_from(records[0].fields()[0].clone()).unwrap();
        let r = Relationship::try_from(records[0].fields()[1].clone()).unwrap();
        let l = Node::try_from(records[0].fields()[2].clone()).unwrap();

        assert_eq!(c.labels(), &["Client".to_string()]);
        assert_eq!(
            c.properties().get("name"),
            Some(&Value::from("bolt-client"))
        );
        assert_eq!(l.labels(), &["Language".to_string()]);
        assert_eq!(l.properties().get("name"), Some(&Value::from("Rust")));
        assert_eq!(r.rel_type(), "WRITTEN_IN");
        assert!(r.properties().is_empty());
        assert_eq!(
            (r.start_node_identity(), r.end_node_identity()),
            (c.node_identity(), l.node_identity())
        );
    }

    #[tokio::test]
    async fn discard_all_fail() {
        let client = get_initialized_client(1).await;
        skip_if_handshake_failed!(client);
        let mut client = client.unwrap();
        let response = client.discard_all().await.unwrap();
        assert!(Failure::try_from(response).is_ok());
    }

    #[tokio::test]
    async fn discard_all() {
        let client = get_initialized_client(1).await;
        skip_if_handshake_failed!(client);
        let mut client = client.unwrap();
        let response = run_valid_query(&mut client).await.unwrap();
        assert!(Success::try_from(response).is_ok());
        let response = client.discard_all().await.unwrap();
        assert!(Success::try_from(response).is_ok());
    }

    #[tokio::test]
    async fn discard_all_and_pull() {
        let client = get_initialized_client(1).await;
        skip_if_handshake_failed!(client);
        let mut client = client.unwrap();
        let response = run_valid_query(&mut client).await.unwrap();
        assert!(Success::try_from(response).is_ok());
        let response = client.discard_all().await.unwrap();
        assert!(Success::try_from(response).is_ok());
        let (response, records) = client.pull_all().await.unwrap();
        assert!(Failure::try_from(response).is_ok());
        assert!(records.is_empty());
    }

    #[tokio::test]
    async fn reset() {
        let client = get_initialized_client(1).await;
        skip_if_handshake_failed!(client);
        let mut client = client.unwrap();
        let response = run_invalid_query(&mut client).await.unwrap();
        assert!(Failure::try_from(response).is_ok());
        let response = run_valid_query(&mut client).await.unwrap();
        assert!(match response {
            Message::Ignored => true,
            _ => false,
        });
        let response = client.reset().await.unwrap();
        assert!(Success::try_from(response).is_ok());
        let response = run_valid_query(&mut client).await.unwrap();
        assert!(Success::try_from(response).is_ok());
    }

    #[tokio::test]
    async fn ignored() {
        let client = get_initialized_client(1).await;
        skip_if_handshake_failed!(client);
        let mut client = client.unwrap();
        let response = run_invalid_query(&mut client).await.unwrap();
        assert!(Failure::try_from(response).is_ok());
        let response = run_valid_query(&mut client).await.unwrap();
        assert!(match response {
            Message::Ignored => true,
            _ => false,
        });
    }

    #[tokio::test]
    async fn v3_method_with_v1_client_fails() {
        let client = get_initialized_client(1).await;
        skip_if_handshake_failed!(client);
        let mut client = client.unwrap();
        assert!(match client.commit().await {
            Err(Error::UnsupportedOperation(Some(1))) => true,
            _ => false,
        });
    }

    #[tokio::test]
    async fn v3_message_with_v1_client_fails() {
        let client = get_initialized_client(1).await;
        skip_if_handshake_failed!(client);
        let mut client = client.unwrap();
        client.send_message(Message::Commit).await.unwrap();
        assert!(match client.read_message().await {
            // Local server just closes connection, but GrapheneDB sends a FAILURE message
            Err(Error::ProtocolError(_)) | Ok(Message::Failure(_)) => true,
            _ => false,
        });
    }
}
