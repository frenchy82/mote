//! The way to the server: straight there over TCP, through an SSH tunnel,
//! or through an RD Gateway, after waking the computer when asked to.

mod gateway;
mod ssh;
mod wake;

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use tokio::net::TcpStream;
use tokio::sync::mpsc;

use super::connect::Stream;
use super::{Event, Failure, Target, Unreachable, Via};

const TCP_TIMEOUT: Duration = Duration::from_secs(15);

/// How long a computer woken over the network gets to come up.
const WAKE_FOR: Duration = Duration::from_secs(120);

/// How often the packet goes out again while it does: one can be lost,
/// and a computer only part way asleep can miss it.
const WAKE_AGAIN: Duration = Duration::from_secs(15);

/// How long each try waits for an answer meanwhile: a computer asleep
/// never answers, so a short wait only tells sooner that it still sleeps.
const WAKE_TRY: Duration = Duration::from_secs(3);

/// A connection to the server, and the address this side of it has.
pub(super) async fn open(
    target: &Target,
    events: &mpsc::Sender<Event>,
) -> Result<(Box<dyn Stream>, SocketAddr), Failure> {
    let Some(mac) = target.wake else {
        return once(target, events, TCP_TIMEOUT).await;
    };
    let until = Instant::now() + WAKE_FOR;
    let mut sent: Option<Instant> = None;
    let mut told = false;
    loop {
        if sent.is_none_or(|at| at.elapsed() >= WAKE_AGAIN) {
            wake::send(mac).await;
            sent = Some(Instant::now());
        }
        match once(target, events, WAKE_TRY).await {
            Err(failure) if failure.asleep() && Instant::now() < until => {
                if !told {
                    told = true;
                    tracing::info!("waiting for {} to wake up", target.host);
                    let _ = events.send(Event::Waking).await;
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            result => return result,
        }
    }
}

async fn once(
    target: &Target,
    events: &mpsc::Sender<Event>,
    patience: Duration,
) -> Result<(Box<dyn Stream>, SocketAddr), Failure> {
    match &target.via {
        Via::Direct => {
            let stream = tcp(&target.host, target.port, patience)
                .await
                .map_err(Failure::Unreachable)?;
            let local = stream
                .local_addr()
                .map_err(|e| Failure::Unreachable(Unreachable::from_io(&e)))?;
            Ok((Box::new(stream), local))
        }
        Via::Ssh(hop) => ssh::open(hop, target, events).await,
        Via::Gateway(hop) => gateway::open(hop, target, events).await,
    }
}

/// A TCP connection that notices a network gone away within half a
/// minute, rather than the quarter of an hour TCP gives a silent one.
async fn tcp(host: &str, port: u16, patience: Duration) -> Result<TcpStream, Unreachable> {
    let stream = match tokio::time::timeout(patience, TcpStream::connect((host, port))).await {
        Ok(Ok(stream)) => stream,
        Ok(Err(e)) => return Err(Unreachable::from_io(&e)),
        Err(_) => return Err(Unreachable::NoAnswer),
    };
    let _ = stream.set_nodelay(true);
    let socket = socket2::SockRef::from(&stream);
    let keepalive = socket2::TcpKeepalive::new()
        .with_time(Duration::from_secs(10))
        .with_interval(Duration::from_secs(5))
        .with_retries(3);
    if let Err(e) = socket
        .set_tcp_keepalive(&keepalive)
        .and_then(|()| socket.set_tcp_user_timeout(Some(Duration::from_secs(20))))
    {
        tracing::debug!("setting the connection's timeouts: {e}");
    }
    Ok(stream)
}
