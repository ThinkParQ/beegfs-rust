use anyhow::Result;
use std::fmt::Write;
use std::future::Future;
use std::pin::Pin;
use tokio::sync::mpsc;
use tokio_stream::Stream;
use tonic::{Code, Status};

/// Autoimplements a gRPC handle function as expected by the auto-defined protobuf traits. It
/// forwards to the actual, user defined, handler function and handles errors returned from the it.
/// One call of the macro implements one function. Must be called from within the trait impl block.
///
/// The handler must return anyhow::Result<$resp_msg> or in case of a stream,
/// anyhow::Result<$resp_stream>. If the result is an error and contains and contains a
/// `AnyhowContextStatus` by the handler calling `.status_code()` on a `Result`, the assigned status
/// code will be sent to the client as given. This can be used to control which gRPC status to send
/// back. For any other error, a generic Code::Internal will be sent back. Both  include the whole
/// stringified error chain. Finally, if the Result is Ok, a tonic::Response containing the returned
/// value will be sent back.
#[macro_export]
macro_rules! impl_grpc_handler {
    // Implements the function for a response stream RPC.
    ($impl_fn:ident => $handle_fn:path, $req_msg:path => STREAM($resp_stream:ident, $resp_msg:path), $ctx_str:literal) => {
        // A response stream RPC requires to define a `<MsgName>Stream` associated type and use this
        // as the response for the handler.
        type $resp_stream = RespStream<$resp_msg>;

        impl_grpc_handler!(@INNER $impl_fn => $handle_fn, $req_msg => Self::$resp_stream, $ctx_str);
    };

    // Implements the function for a unary RPC.
    ($impl_fn:ident => $handle_fn:path, $req_msg:path => $resp_msg:path, $ctx_str:literal) => {
        impl_grpc_handler!(@INNER $impl_fn => $handle_fn, $req_msg => $resp_msg, $ctx_str);
    };

    // Generates the actual function. Note that we implement the `async fn` manually to avoid having
    // to use `#[tonic::async_trait]`. This is exactly how that macro does it in the background, but
    // we can't rely on that here within this macro as attribute macros are evaluated first.
    (@INNER $impl_fn:ident => $handle_fn:path, $req_msg:path => $resp_msg:path, $ctx_str:literal) => {
        fn $impl_fn<'a, 'async_trait>(
            &'a self,
            req: Request<$req_msg>,
        ) -> Pin<Box<dyn Future<Output = Result<Response<$resp_msg>, Status>> + Send + 'async_trait>>
        where
            'a: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async move {
                let res = $handle_fn(self.ctx.clone(), req.into_inner()).await;

                match res {
                    Ok(res) => Ok(Response::new(res)),
                    Err(err) => {
                        let status = $crate::grpc::process_grpc_handler_error(err.context($ctx_str));
                        Err(status)
                    }
                }
            })
        }
    };
}

// RESPONSE STREAM

/// Wrapper around the stream channel sender
#[derive(Debug, Clone)]
pub struct StreamSender<Msg>(mpsc::Sender<std::result::Result<Msg, Status>>);

impl<Msg: Send + Sync + 'static> StreamSender<Msg> {
    /// Sends a msg to the stream
    pub async fn send(&self, value: Msg) -> Result<()> {
        self.0.send(Ok(value)).await?;
        Ok(())
    }
}

/// Convenience alias for the gRPC response stream future
pub type RespStream<T> = Pin<Box<dyn Stream<Item = std::result::Result<T, Status>> + Send>>;

/// Stream back response messages to the requester if the rpc expects it.
/// Provide the source_fn function/closure to generate the streams results.
///
/// The source_fn function accepts anyhow::Result as return value. If the result is an error and
/// contains and contains a `AnyhowContextStatus` by the handler calling `.status_code()` on a
/// `Result`, the assigned status code will be sent to the client as given. This can be used to
/// control which gRPC status to send back. For any other error, a generic Code::Internal will be
/// sent back. Both  include the whole stringified error chain. Finally, if the Result is Ok, the
/// stream will close normally (meaning "success").
///
/// buf_size determines the size of the ready-to-send message buffer. For most callers, especially
/// those who obtain all the needed data at once, this doesn't make a big difference and a small
/// buffer size like 16 should do it - if the response messages are already generated in memory,
/// there is likely no benefit from an extra buffer. Putting a big number might even hurt in this
/// case as the big allocation takes extra effort.
/// However, if data is fetched and streamed page after page, it's a different story. Depending
/// on how long it takes to fetch and generate each page of response messages, a big number can
/// make sense to allow the task to already start fetching the next page while the current one is
/// still being streamed. If there are moments during request handling where the buffer runs out of
/// sendable messages while fetching the next page of data, increasing the buffer increases
/// throughput. On the other hand, if most requests only fetch a fraction of a max page size, a big
/// buffer doesn't really make sense, it just wastes memory. The implementor should take that into
/// account.
pub fn resp_stream<RespMsg, SourceFn, Fut>(
    buf_size: usize,
    source_fn: SourceFn,
) -> RespStream<RespMsg>
where
    RespMsg: Send + Sync + 'static,
    SourceFn: FnOnce(StreamSender<RespMsg>) -> Fut + Send + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    let (tx, rx) = mpsc::channel(buf_size.max(1));

    tokio::spawn(async move {
        if let Err(err) = source_fn(StreamSender(tx.clone())).await {
            let msg_name = std::any::type_name::<RespMsg>()
                .split("::")
                .last()
                .expect("RespMsg implementor name is empty");
            // If this is the result of a closed receive channel (e.g. client cancels
            // receiving early), we don't want to log this as an error.
            if err.is::<mpsc::error::SendError<std::result::Result<RespMsg, Status>>>() {
                log::debug!(
                    "response stream of {msg_name} got interrupted: receiver closed the channel"
                );
                return;
            }

            let status =
                process_grpc_handler_error(err.context(format!("Response stream of {msg_name}",)));
            let _ = tx.send(Err(status)).await;
        }
    });

    let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    Box::pin(stream)
}

// ANYHOW CONTEXT STATUS

/// A wrapper around an inner anyhow::Error, containing additional gRPC status code info. When
/// handling an error chain, the generic `dyn Error` can be downcasted and the status code can be
/// extracted. When logging errors, this also allows skipping this status code item as you normally
/// don't want to print this. This is also the reason why we don't just use anyhows `.context()` as
/// downcasting on individual errors in the chain does not work there.
#[derive(Debug)]
pub struct AnyhowContextStatus {
    status_code: Code,
    source: Option<anyhow::Error>,
}

impl std::error::Error for AnyhowContextStatus {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source.as_ref().map(|s| s.as_ref())
    }
}

/// Note that usually, when printing handler results, this should not be printed as this is actually
/// not real error context (the whole point of this custom wrapper). If put into `format!("{:#}")`
/// or similar, the default print logic will print it though as part of the chain. For that case,
/// we still add a reasonable output here.
impl std::fmt::Display for AnyhowContextStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "[Set status code: {:?}]", self.status_code)
    }
}

pub trait AnyhowErrorStatusExt {
    type Ok;
    /// Attach a gRPC status code to the error chain which can be extracted by the error handler.
    /// Meant to be used for determining which error code to send back to the client.
    fn status_code(self, status_code: Code) -> Result<Self::Ok, anyhow::Error>;
}

impl<T, E> AnyhowErrorStatusExt for std::result::Result<T, E>
where
    E: Into<anyhow::Error>,
{
    type Ok = T;

    fn status_code(self, status_code: Code) -> Result<T, anyhow::Error> {
        self.map_err(|err| {
            anyhow::Error::new(AnyhowContextStatus {
                status_code,
                source: Some(err.into()),
            })
        })
    }
}

// UTIL

/// Logs an error returned from a gRPC handler and extracts the set response status code or
/// defaults to unknown (including the stringified error chain).
pub fn process_grpc_handler_error(err: anyhow::Error) -> Status {
    let mut resp_code = Code::Unknown;
    let mut err_string = String::new();

    let mut delim = "";
    for s in err.chain() {
        if let Some(d) = s.downcast_ref::<AnyhowContextStatus>() {
            resp_code = d.status_code;
            continue;
        }

        write!(err_string, "{delim}{s}").ok();
        delim = ": ";
    }

    log::error!("[{resp_code:?}]: {err_string}");

    Status::new(resp_code, err_string)
}

/// Unwraps an optional proto message field . If `None`, errors out providing the fields name in the
/// error message.
///
/// Meant for unwrapping optional protobuf fields that are actually mandatory.
pub fn required_field<T>(f: Option<T>) -> Result<T> {
    f.ok_or_else(|| ::anyhow::anyhow!("missing required {} field", std::any::type_name::<T>()))
}
