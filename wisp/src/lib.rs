#![feature(impl_trait_in_assoc_type)]
#[cfg(feature = "fastwebsockets")]
mod fastwebsockets;
mod packet;
mod stream;
#[cfg(feature = "hyper_tower")]
pub mod tokioio;
#[cfg(feature = "hyper_tower")]
pub mod tower;
pub mod ws;
#[cfg(feature = "ws_stream_wasm")]
mod ws_stream_wasm;

pub use crate::packet::*;
pub use crate::stream::*;

use event_listener::Event;
use futures::{channel::mpsc, lock::Mutex, Future, FutureExt, StreamExt};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc,
    },
};

#[derive(Debug, PartialEq, Copy, Clone)]
pub enum Role {
    Client,
    Server,
}

#[derive(Debug)]
pub enum WispError {
    PacketTooSmall,
    InvalidPacketType,
    InvalidStreamType,
    InvalidStreamId,
    InvalidUri,
    UriHasNoHost,
    UriHasNoPort,
    MaxStreamCountReached,
    StreamAlreadyClosed,
    WsFrameInvalidType,
    WsFrameNotFinished,
    WsImplError(Box<dyn std::error::Error + Sync + Send>),
    WsImplSocketClosed,
    WsImplNotSupported,
    Utf8Error(std::str::Utf8Error),
    Other(Box<dyn std::error::Error + Sync + Send>),
}

impl From<std::str::Utf8Error> for WispError {
    fn from(err: std::str::Utf8Error) -> WispError {
        WispError::Utf8Error(err)
    }
}

impl std::fmt::Display for WispError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        use WispError::*;
        match self {
            PacketTooSmall => write!(f, "Packet too small"),
            InvalidPacketType => write!(f, "Invalid packet type"),
            InvalidStreamType => write!(f, "Invalid stream type"),
            InvalidStreamId => write!(f, "Invalid stream id"),
            InvalidUri => write!(f, "Invalid URI"),
            UriHasNoHost => write!(f, "URI has no host"),
            UriHasNoPort => write!(f, "URI has no port"),
            MaxStreamCountReached => write!(f, "Maximum stream count reached"),
            StreamAlreadyClosed => write!(f, "Stream already closed"),
            WsFrameInvalidType => write!(f, "Invalid websocket frame type"),
            WsFrameNotFinished => write!(f, "Unfinished websocket frame"),
            WsImplError(err) => write!(f, "Websocket implementation error: {:?}", err),
            WsImplSocketClosed => write!(f, "Websocket implementation error: websocket closed"),
            WsImplNotSupported => write!(f, "Websocket implementation error: unsupported feature"),
            Utf8Error(err) => write!(f, "UTF-8 error: {:?}", err),
            Other(err) => write!(f, "Other error: {:?}", err),
        }
    }
}

impl std::error::Error for WispError {}

struct ServerMuxInner<W>
where
    W: ws::WebSocketWrite + Send + 'static,
{
    tx: ws::LockedWebSocketWrite<W>,
    stream_map: Arc<Mutex<HashMap<u32, mpsc::UnboundedSender<WsEvent>>>>,
    close_tx: mpsc::UnboundedSender<MuxEvent>,
}

impl<W: ws::WebSocketWrite + Send + 'static> ServerMuxInner<W> {
    pub async fn into_future<R>(
        self,
        rx: R,
        close_rx: mpsc::UnboundedReceiver<MuxEvent>,
        muxstream_sender: mpsc::UnboundedSender<(ConnectPacket, MuxStream<W>)>,
        buffer_size: u32
    ) -> Result<(), WispError>
    where
        R: ws::WebSocketRead,
    {
        let ret = futures::select! {
            x = self.server_close_loop(close_rx, self.stream_map.clone(), self.tx.clone()).fuse() => x,
            x = self.server_msg_loop(rx, muxstream_sender, buffer_size).fuse() => x
        };
        self.stream_map.lock().await.iter().for_each(|x| {
            let _ = x.1.unbounded_send(WsEvent::Close(ClosePacket::new(0x01)));
        });
        ret
    }

    async fn server_close_loop(
        &self,
        mut close_rx: mpsc::UnboundedReceiver<MuxEvent>,
        stream_map: Arc<Mutex<HashMap<u32, mpsc::UnboundedSender<WsEvent>>>>,
        tx: ws::LockedWebSocketWrite<W>,
    ) -> Result<(), WispError> {
        while let Some(msg) = close_rx.next().await {
            match msg {
                MuxEvent::Close(stream_id, reason, channel) => {
                    if stream_map.lock().await.remove(&stream_id).is_some() {
                        let _ = channel.send(
                            tx.write_frame(Packet::new_close(stream_id, reason).into())
                                .await,
                        );
                    } else {
                        let _ = channel.send(Err(WispError::InvalidStreamId));
                    }
                }
            }
        }
        Ok(())
    }

    async fn server_msg_loop<R>(
        &self,
        mut rx: R,
        muxstream_sender: mpsc::UnboundedSender<(ConnectPacket, MuxStream<W>)>,
        buffer_size: u32,
    ) -> Result<(), WispError>
    where
        R: ws::WebSocketRead,
    {
        self.tx
            .write_frame(Packet::new_continue(0, buffer_size).into())
            .await?;

        while let Ok(frame) = rx.wisp_read_frame(&self.tx).await {
            if let Ok(packet) = Packet::try_from(frame) {
                use PacketType::*;
                match packet.packet {
                    Connect(inner_packet) => {
                        let (ch_tx, ch_rx) = mpsc::unbounded();
                        self.stream_map.lock().await.insert(packet.stream_id, ch_tx);
                        muxstream_sender
                            .unbounded_send((
                                inner_packet,
                                MuxStream::new(
                                    packet.stream_id,
                                    Role::Server,
                                    ch_rx,
                                    self.tx.clone(),
                                    self.close_tx.clone(),
                                    AtomicBool::new(false).into(),
                                    AtomicU32::new(buffer_size).into(),
                                    Event::new().into(),
                                ),
                            ))
                            .map_err(|x| WispError::Other(Box::new(x)))?;
                    }
                    Data(data) => {
                        if let Some(stream) = self.stream_map.lock().await.get(&packet.stream_id) {
                            let _ = stream.unbounded_send(WsEvent::Send(data));
                        }
                    }
                    Continue(_) => unreachable!(),
                    Close(inner_packet) => {
                        if let Some(stream) = self.stream_map.lock().await.get(&packet.stream_id) {
                            let _ = stream.unbounded_send(WsEvent::Close(inner_packet));
                        }
                        self.stream_map.lock().await.remove(&packet.stream_id);
                    }
                }
            } else {
                break;
            }
        }
        drop(muxstream_sender);
        Ok(())
    }
}

pub struct ServerMux<W>
where
    W: ws::WebSocketWrite + Send + 'static,
{
    muxstream_recv: mpsc::UnboundedReceiver<(ConnectPacket, MuxStream<W>)>,
}

impl<W: ws::WebSocketWrite + Send + 'static> ServerMux<W> {
    pub fn new<R>(read: R, write: W, buffer_size: u32) -> (Self, impl Future<Output = Result<(), WispError>>)
    where
        R: ws::WebSocketRead,
    {
        let (close_tx, close_rx) = mpsc::unbounded::<MuxEvent>();
        let (tx, rx) = mpsc::unbounded::<(ConnectPacket, MuxStream<W>)>();
        let write = ws::LockedWebSocketWrite::new(write);
        let map = Arc::new(Mutex::new(HashMap::new()));
        (
            Self { muxstream_recv: rx },
            ServerMuxInner {
                tx: write,
                close_tx,
                stream_map: map.clone(),
            }
            .into_future(read, close_rx, tx, buffer_size),
        )
    }

    pub async fn server_new_stream(&mut self) -> Option<(ConnectPacket, MuxStream<W>)> {
        self.muxstream_recv.next().await
    }
}

pub struct ClientMuxInner<W>
where
    W: ws::WebSocketWrite,
{
    tx: ws::LockedWebSocketWrite<W>,
    stream_map:
        Arc<Mutex<HashMap<u32, (mpsc::UnboundedSender<WsEvent>, Arc<AtomicU32>, Arc<Event>)>>>,
}

impl<W: ws::WebSocketWrite + Send> ClientMuxInner<W> {
    pub async fn into_future<R>(
        self,
        rx: R,
        close_rx: mpsc::UnboundedReceiver<MuxEvent>,
    ) -> Result<(), WispError>
    where
        R: ws::WebSocketRead,
    {
        futures::select! {
            x = self.client_bg_loop(close_rx).fuse() => x,
            x = self.client_loop(rx).fuse() => x
        }
    }

    async fn client_bg_loop(
        &self,
        mut close_rx: mpsc::UnboundedReceiver<MuxEvent>,
    ) -> Result<(), WispError> {
        while let Some(msg) = close_rx.next().await {
            match msg {
                MuxEvent::Close(stream_id, reason, channel) => {
                    if self.stream_map.lock().await.remove(&stream_id).is_some() {
                        let _ = channel.send(
                            self.tx
                                .write_frame(Packet::new_close(stream_id, reason).into())
                                .await,
                        );
                    } else {
                        let _ = channel.send(Err(WispError::InvalidStreamId));
                    }
                }
            }
        }
        Ok(())
    }

    async fn client_loop<R>(&self, mut rx: R) -> Result<(), WispError>
    where
        R: ws::WebSocketRead,
    {
        while let Ok(frame) = rx.wisp_read_frame(&self.tx).await {
            if let Ok(packet) = Packet::try_from(frame) {
                use PacketType::*;
                match packet.packet {
                    Connect(_) => unreachable!(),
                    Data(data) => {
                        if let Some(stream) = self.stream_map.lock().await.get(&packet.stream_id) {
                            let _ = stream.0.unbounded_send(WsEvent::Send(data));
                        }
                    }
                    Continue(inner_packet) => {
                        if let Some(stream) = self.stream_map.lock().await.get(&packet.stream_id) {
                            stream
                                .1
                                .store(inner_packet.buffer_remaining, Ordering::Release);
                            let _ = stream.2.notify(u32::MAX);
                        }
                    }
                    Close(inner_packet) => {
                        if let Some(stream) = self.stream_map.lock().await.get(&packet.stream_id) {
                            let _ = stream.0.unbounded_send(WsEvent::Close(inner_packet));
                        }
                        self.stream_map.lock().await.remove(&packet.stream_id);
                    }
                }
            }
        }
        Ok(())
    }
}

pub struct ClientMux<W>
where
    W: ws::WebSocketWrite,
{
    tx: ws::LockedWebSocketWrite<W>,
    stream_map:
        Arc<Mutex<HashMap<u32, (mpsc::UnboundedSender<WsEvent>, Arc<AtomicU32>, Arc<Event>)>>>,
    next_free_stream_id: AtomicU32,
    close_tx: mpsc::UnboundedSender<MuxEvent>,
    buf_size: u32,
}

impl<W: ws::WebSocketWrite + Send + 'static> ClientMux<W> {
    pub async fn new<R>(
        mut read: R,
        write: W,
    ) -> Result<(Self, impl Future<Output = Result<(), WispError>>), WispError>
    where
        R: ws::WebSocketRead,
    {
        let write = ws::LockedWebSocketWrite::new(write);
        let first_packet = Packet::try_from(read.wisp_read_frame(&write).await?)?;
        if first_packet.stream_id != 0 {
            return Err(WispError::InvalidStreamId);
        }
        if let PacketType::Continue(packet) = first_packet.packet {
            let (tx, rx) = mpsc::unbounded::<MuxEvent>();
            let map = Arc::new(Mutex::new(HashMap::new()));
            Ok((
                Self {
                    tx: write.clone(),
                    stream_map: map.clone(),
                    next_free_stream_id: AtomicU32::new(1),
                    close_tx: tx,
                    buf_size: packet.buffer_remaining,
                },
                ClientMuxInner {
                    tx: write.clone(),
                    stream_map: map.clone(),
                }
                .into_future(read, rx),
            ))
        } else {
            Err(WispError::InvalidPacketType)
        }
    }

    pub async fn client_new_stream(
        &self,
        stream_type: StreamType,
        host: String,
        port: u16,
    ) -> Result<MuxStream<W>, WispError> {
        let (ch_tx, ch_rx) = mpsc::unbounded();
        let evt: Arc<Event> = Event::new().into();
        let flow_control: Arc<AtomicU32> = AtomicU32::new(self.buf_size).into();
        let stream_id = self.next_free_stream_id.load(Ordering::Acquire);
        self.tx
            .write_frame(Packet::new_connect(stream_id, stream_type, port, host).into())
            .await?;
        self.next_free_stream_id.store(
            stream_id
                .checked_add(1)
                .ok_or(WispError::MaxStreamCountReached)?,
            Ordering::Release,
        );
        self.stream_map
            .lock()
            .await
            .insert(stream_id, (ch_tx, flow_control.clone(), evt.clone()));
        Ok(MuxStream::new(
            stream_id,
            Role::Client,
            ch_rx,
            self.tx.clone(),
            self.close_tx.clone(),
            AtomicBool::new(false).into(),
            flow_control,
            evt,
        ))
    }
}
