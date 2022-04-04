//! A Lavalink and Andesite API wrapper library for every tokio based discord bot library.
#![deny(clippy::unused_async, clippy::await_holding_lock)]
#![warn(clippy::pedantic, clippy::future_not_send)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::missing_panics_doc,
    clippy::missing_errors_doc,
    clippy::cast_sign_loss,
    clippy::wildcard_imports
)]

#[cfg(feature = "tracing-log")]
#[macro_use]
extern crate tracing;

#[cfg(feature = "normal-log")]
#[macro_use]
extern crate log;

/// Builder structures
pub mod builders;
/// Library's errors
pub mod error;
mod event_loops;
/// Gateway events
pub mod gateway;
/// Library models
pub mod model;
#[cfg(feature = "discord-gateway")]
/// Voice connection handling
pub mod voice;

/// Re-export to be used with the event handler.
pub use async_trait::async_trait;
use tokio::sync::mpsc::UnboundedSender;
/// Re-export to be used with the Node data.
pub use typemap_rev;

use builders::*;
use error::LavalinkError;
use error::LavalinkResult;

#[cfg(feature = "discord-gateway")]
use event_loops::discord_event_loop;
use event_loops::lavalink_event_loop;

use gateway::LavalinkEventHandler;
use model::*;

use std::collections::HashMap;
use std::collections::HashSet;
use std::{
    cmp::{max, min},
    sync::Arc,
    time::Duration,
};

#[cfg(feature = "songbird")]
use songbird_dep::ConnectionInfo as SongbirdConnectionInfo;

use reqwest::{header::HeaderMap, Client as ReqwestClient, Url};

#[cfg(feature = "native")]
use tokio_native_tls::TlsStream;
#[cfg(feature = "rustls")]
use tokio_rustls::client::TlsStream;

use parking_lot::Mutex;
use tokio::net::TcpStream;

use regex::Regex;

use async_tungstenite::{
    stream::Stream, tokio::TokioAdapter, tungstenite::Message as TungsteniteMessage,
    WebSocketStream,
};

#[cfg(feature = "discord-gateway")]
use tokio::sync::mpsc;

use dashmap::DashMap;

/// All 0's equalizer preset. Default.
pub const EQ_BASE: [f64; 15] = [
    0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];

/// Basic boost equalizer with higher lows and highs.
pub const EQ_BOOST: [f64; 15] = [
    -0.075, 0.125, 0.125, 0.1, 0.1, 0.05, 0.075, 0.0, 0.0, 0.0, 0.0, 0.0, 0.125, 0.15, 0.05,
];
/// Equalizer preset for most metal music.
pub const EQ_METAL: [f64; 15] = [
    0.0, 0.1, 0.1, 0.15, 0.13, 0.1, 0.0, 0.125, 0.175, 0.175, 0.125, 0.125, 0.1, 0.075, 0.0,
];
/// Equalizer preset for piano and classical.
pub const EQ_PIANO: [f64; 15] = [
    -0.25, -0.25, -0.125, 0.0, 0.25, 0.25, 0.0, -0.25, -0.25, 0.0, 0.0, 0.5, 0.25, -0.025, 0.0,
];

pub type WsStream =
    WebSocketStream<Stream<TokioAdapter<TcpStream>, TokioAdapter<TlsStream<TcpStream>>>>;

/// NOTE: All fields are public for those who want to do their own implementation of things, you
/// should not be touching them if you don't know what you are doing.
pub struct LavalinkClientInner {
    //pub socket_uri: String,
    pub rest_uri: String,
    pub headers: HeaderMap,

    /// The sender websocket split.
    pub socket_write: Option<UnboundedSender<TungsteniteMessage>>,
    pub socket_uri: String,

    //_shard_id: Option<ShardId>,
    pub nodes: HashMap<u64, Node>,
    pub loops: HashSet<u64>,

    #[cfg(feature = "discord-gateway")]
    pub discord_gateway_data: DiscordGatewayData,
    // Unused
    //_region: Option<Region>,
    //_identifier: Option<String>,
}

#[cfg(feature = "discord-gateway")]
#[derive(Clone)]
pub struct DiscordGatewayData {
    pub shard_count: u64,
    pub bot_id: UserId,
    pub bot_token: String,
    pub wait_time: Duration,
    pub headers: HeaderMap,
    pub sender: mpsc::UnboundedSender<String>,
    pub connections: Arc<DashMap<GuildId, ConnectionInfo>>,
    pub socket_uri: &'static str,
}

/// A Client for Lavalink.
///
/// This structure is behind `Arc`, so it's clone and thread safe.
///
/// The inner field is public for those who want to tinker with it manually.
#[derive(Clone)]
pub struct LavalinkClient {
    /// Field is public for those who want to do their own implementation of things.
    pub inner: Arc<Mutex<LavalinkClientInner>>,
}

impl LavalinkClient {
    /// Builds the Client connection.
    pub async fn new(
        builder: &LavalinkClientBuilder,
        handler: impl LavalinkEventHandler + Send + Sync + 'static,
    ) -> LavalinkResult<Self> {
        let (lavalink_headers, lavalink_rest_uri, lavalink_socket_uri) = {
            let socket_uri;
            let rest_uri;

            if builder.is_ssl {
                socket_uri = format!("wss://{}:{}", &builder.host, builder.port);
                rest_uri = format!("https://{}:{}", &builder.host, builder.port);
            } else {
                socket_uri = format!("ws://{}:{}", &builder.host, builder.port);
                rest_uri = format!("http://{}:{}", &builder.host, builder.port);
            }

            let mut headers = HeaderMap::new();
            headers.insert("Authorization", builder.password.parse()?);
            headers.insert("Num-Shards", builder.shard_count.to_string().parse()?);
            headers.insert("User-Id", builder.bot_id.to_string().parse()?);
            headers.insert(
                "Client-Name",
                concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"))
                    .to_owned()
                    .parse()?,
            );

            (headers, rest_uri, socket_uri)
        };

        #[cfg(feature = "discord-gateway")]
        let (discord_socket_uri, discord_headers) = {
            let socket_uri = "wss://gateway.discord.gg/?v=9&encoding=json";

            let mut headers = HeaderMap::new();
            headers.insert(
                "Authorization",
                format!("Bot {}", builder.bot_token).parse()?,
            );
            headers.insert("bot", "True".to_string().parse()?);
            headers.insert("Content-type", "application/json".to_string().parse()?);

            (socket_uri, headers)
        };

        #[cfg(feature = "discord-gateway")]
        let discord_gateway_data = {
            DiscordGatewayData {
                shard_count: builder.shard_count,
                bot_id: builder.bot_id,
                bot_token: builder.bot_token.to_string(),
                wait_time: builder.gateway_start_wait_time,
                headers: discord_headers,
                sender: mpsc::unbounded_channel().0,
                connections: Arc::new(DashMap::new()),
                socket_uri: discord_socket_uri,
            }
        };

        let client_inner = LavalinkClientInner {
            headers: lavalink_headers,
            socket_write: None,
            rest_uri: lavalink_rest_uri,
            nodes: HashMap::new(),
            loops: HashSet::new(),
            socket_uri: lavalink_socket_uri,
            #[cfg(feature = "discord-gateway")]
            discord_gateway_data,
        };

        let client = Self {
            inner: Arc::new(Mutex::new(client_inner)),
        };

        let client_clone = client.clone();

        tokio::spawn(async move {
            lavalink_event_loop(handler, client_clone).await;
        });

        #[cfg(feature = "discord-gateway")]
        if builder.start_gateway {
            let client_clone = client.clone();
            let token = builder.bot_token.clone();
            let wait_time = builder.gateway_start_wait_time;

            tokio::spawn(async move {
                debug!("Starting discord event loop.");
                discord_event_loop(client_clone, &token, wait_time).await;
                error!("Event loop ended unexpectedly.");
            });
        }

        Ok(client)
    }

    /// Returns the inner raw message sender for Lavalink.
    ///
    /// # Warning
    /// This is just an interface into the [`LavalinkClientInner`] attribute, so any warnings on that apply.
    pub fn socket_write(&self) -> LavalinkResult<UnboundedSender<TungsteniteMessage>> {
        self.inner
            .lock()
            .socket_write
            .as_ref()
            .cloned()
            .ok_or(LavalinkError::MissingLavalinkSocket)
    }

    /// Returns a builder to be used to create a Client.
    ///
    /// ```rust
    /// struct LavalinkHandler;
    ///
    /// #[async_trait]
    /// impl LavalinkEventHandler for LavalinkHandler {
    ///     async fn track_start(&self, _client: LavalinkClient, event: TrackStart) {
    ///         info!("Track started!\nGuild: {}", event.guild_id);
    ///     }
    ///     async fn track_finish(&self, _client: LavalinkClient, event: TrackFinish) {
    ///         info!("Track finished!\nGuild: {}", event.guild_id);
    ///     }
    /// }
    ///     
    /// let lavalink_client = LavalinkClient::builder(bot_id)
    ///     .set_host("127.0.0.1")
    ///     .set_password(env::var("LAVALINK_PASSWORD").unwrap_or("youshallnotpass".to_string()))
    ///     .build(LavalinkHandler)
    ///     .await?;
    /// ```
    #[cfg(feature = "discord-gateway")]
    pub fn builder(
        user_id: impl Into<UserId>,
        bot_token: impl Into<String>,
    ) -> LavalinkClientBuilder {
        LavalinkClientBuilder::new(user_id, bot_token)
    }
    #[cfg(not(feature = "discord-gateway"))]
    pub fn builder(user_id: impl Into<UserId>) -> LavalinkClientBuilder {
        LavalinkClientBuilder::new(user_id)
    }

    /// Start the discord gateway, if it has stopped, or it never started because the client builder was
    /// configured that way.
    ///
    /// If `wait_time` is passed, it will override the previosuly configured wait time.
    #[cfg(feature = "discord-gateway")]
    pub async fn start_discord_gateway(&self, wait_time: Option<Duration>) {
        let client_clone = self.clone();
        let gw_data = self.discord_gateway_data();
        let wait_time = if let Some(t) = wait_time {
            t
        } else {
            gw_data.wait_time
        };

        let token = gw_data.bot_token;
        tokio::spawn(async move {
            debug!("Starting discord event loop.");
            discord_event_loop(client_clone, &token, wait_time).await;
            error!("Event loop ended unexpectedly.");
        });
    }

    /// Returns the tracks from the URL or query provided.
    pub async fn get_tracks(&self, query: impl ToString + Send) -> LavalinkResult<Tracks> {
        let (rest_uri, headers) = {
            let client = self.inner.lock();
            (client.rest_uri.to_string(), client.headers.clone())
        };

        let reqwest = ReqwestClient::new();
        let url = Url::parse_with_params(
            &format!("{}/loadtracks", rest_uri),
            &[("identifier", &query.to_string())],
        )
        .expect("The query cannot be formatted to a url.");

        let raw_resp = reqwest.get(url).headers(headers).send().await?;

        let resp = raw_resp.json::<Tracks>().await?;

        Ok(resp)
    }

    /// Will automatically search the query on youtube if it's not a valid URL.
    pub async fn auto_search_tracks(
        &self,
        query: impl ToString + Sync + Send,
    ) -> LavalinkResult<Tracks> {
        let r = Regex::new(r"https?://(?:www\.)?.+").unwrap();
        if r.is_match(&query.to_string()) {
            self.get_tracks(query.to_string()).await
        } else {
            self.get_tracks(format!("ytsearch:{}", query.to_string()))
                .await
        }
    }

    /// Returns tracks from the search query.
    /// Uses youtube to search.
    pub async fn search_tracks(&self, query: impl ToString + Send) -> LavalinkResult<Tracks> {
        self.get_tracks(format!("ytsearch:{}", query.to_string()))
            .await
    }

    /// Decodes a track to it's information
    pub async fn decode_track(&self, track: impl ToString + Send) -> LavalinkResult<Info> {
        let (rest_uri, headers) = {
            let client = self.inner.lock();
            (client.rest_uri.to_string(), client.headers.clone())
        };

        let reqwest = ReqwestClient::new();
        let url = Url::parse_with_params(
            &format!("{}/decodetrack", &rest_uri),
            &[("track", &track.to_string())],
        )
        .expect("The query cannot be formatted to a url.");

        let resp = reqwest
            .get(url)
            .headers(headers)
            .send()
            .await?
            .json::<Info>()
            .await?;

        Ok(resp)
    }

    /// Creates a lavalink session on the specified guild.
    ///
    /// This also creates a Node and inserts it. The node is not added on loops unless
    /// `Play::queue()` is ran.
    #[cfg(feature = "songbird")]
    pub async fn create_session_with_songbird(
        &self,
        connection_info: &SongbirdConnectionInfo,
    ) -> LavalinkResult<()> {
        let event = crate::model::Event {
            token: connection_info.token.to_string(),
            endpoint: connection_info.endpoint.to_string(),
            guild_id: connection_info.guild_id.0.to_string(),
        };

        let payload = crate::model::VoiceUpdate {
            session_id: connection_info.session_id.to_string(),
            event,
        };

        crate::model::SendOpcode::VoiceUpdate(payload)
            .send(connection_info.guild_id, &self.socket_write()?)
            .await?;

        let mut client = self.inner.lock();
        client
            .nodes
            .entry(connection_info.guild_id.0)
            .or_insert_with(Node::default);

        Ok(())
    }
    #[cfg(feature = "discord-gateway")]
    pub async fn create_session(&self, connection_info: &ConnectionInfo) -> LavalinkResult<()> {
        let token = connection_info
            .token
            .as_ref()
            .ok_or(LavalinkError::MissingConnectionField("token"))?
            .to_string();
        let endpoint = connection_info
            .endpoint
            .as_ref()
            .ok_or(LavalinkError::MissingConnectionField("endpoint"))?
            .to_string();
        let guild_id = connection_info
            .guild_id
            .as_ref()
            .ok_or(LavalinkError::MissingConnectionField("guild_id"))?
            .to_string();
        let session_id = connection_info
            .session_id
            .as_ref()
            .ok_or(LavalinkError::MissingConnectionField("session_id"))?
            .to_string();

        let endpoint = if endpoint.starts_with("wss://") {
            endpoint.strip_prefix("wss://").unwrap().to_string()
        } else {
            endpoint
        };

        let event = crate::model::Event {
            token,
            endpoint,
            guild_id,
        };

        let payload = crate::model::VoiceUpdate { session_id, event };

        crate::model::SendOpcode::VoiceUpdate(payload)
            .send(connection_info.guild_id.unwrap(), &self.socket_write()?)
            .await?;

        let mut client = self.inner.lock();
        client
            .nodes
            .entry(connection_info.guild_id.unwrap().0)
            .or_insert_with(Node::default);

        Ok(())
    }

    /// Constructor for playing a track.
    pub fn play(&self, guild_id: impl Into<GuildId>, track: Track) -> PlayParameters {
        PlayParameters {
            track,
            guild_id: guild_id.into().0,
            client: self.clone(),
            replace: false,
            start: 0,
            finish: 0,
            requester: None,
        }
    }

    /// Destroys the current player.
    /// When this is ran, `create_session()` needs to be ran again.
    ///
    /// This method does not remove the guild from the running event loops, nor does it clear the
    /// Node, this allows for reconnecting without losing data.
    /// If you are having issues with disconnecting and reconnecting the bot to a voice channel,
    /// remove the guild from the running event loops and reset the nodes.
    ///
    /// The running loops and the nodes can be obtained via `LavalinkClient::nodes()` and
    /// `LavalinkClient::loops()`
    ///
    /// ```rust,untested
    /// lavalink_client.destroy(guild_id).await?;
    ///
    /// {
    ///     let nodes = lavalink_client.nodes().await;
    ///     nodes.remove(&guild_id.0);
    ///     
    ///     let loops = lavalink_client.loops().await;
    ///     loops.remove(&guild_id.0);
    /// }
    /// ```
    pub async fn destroy(&self, guild_id: impl Into<GuildId> + Send) -> LavalinkResult<()> {
        let guild_id = guild_id.into();

        {
            let mut client = self.inner.lock();

            if let Some(mut node) = client.nodes.get_mut(&guild_id.0) {
                node.now_playing = None;

                if !node.queue.is_empty() {
                    node.queue.remove(0);
                }
            };
        }

        crate::model::SendOpcode::Destroy
            .send(guild_id, &self.socket_write()?)
            .await?;

        Ok(())
    }

    /// Stops the current player.
    pub async fn stop(&self, guild_id: impl Into<GuildId> + Send) -> LavalinkResult<()> {
        crate::model::SendOpcode::Stop
            .send(guild_id, &self.socket_write()?)
            .await?;

        Ok(())
    }

    /// Skips the current playing track to the next item on the queue.
    ///
    /// If nothing is in the queue, the currently playing track will keep playing.
    /// Check if the queue is empty and run `stop()` if that's the case.
    pub async fn skip(&self, guild_id: impl Into<GuildId> + Send) -> Option<TrackQueue> {
        let mut client = self.inner.lock();
        let mut node = client.nodes.get_mut(&guild_id.into().0)?;

        node.now_playing = None;

        if node.queue.is_empty() {
            None
        } else {
            Some(node.queue.remove(0))
        }
    }

    /// Sets the pause status.
    pub async fn set_pause(
        &self,
        guild_id: impl Into<GuildId> + Send,
        pause: bool,
    ) -> LavalinkResult<()> {
        let guild_id = guild_id.into().0;
        let payload = crate::model::Pause { pause };

        {
            let mut client = self.inner.lock();
            let node = client.nodes.get_mut(&guild_id);
            if let Some(mut n) = node {
                n.is_paused = pause;
            }
        }

        crate::model::SendOpcode::Pause(payload)
            .send(guild_id, &self.socket_write()?)
            .await?;

        Ok(())
    }

    /// Sets pause status to `True`
    pub async fn pause(&self, guild_id: impl Into<GuildId> + Send) -> LavalinkResult<()> {
        self.set_pause(guild_id, true).await
    }

    /// Sets pause status to `False`
    pub async fn resume(&self, guild_id: impl Into<GuildId> + Send) -> LavalinkResult<()> {
        self.set_pause(guild_id, false).await
    }

    /// Jumps to a specific time in the currently playing track.
    pub async fn seek(
        &self,
        guild_id: impl Into<GuildId> + Send,
        time: Duration,
    ) -> LavalinkResult<()> {
        let payload = crate::model::Seek {
            position: time.as_millis() as u64,
        };

        crate::model::SendOpcode::Seek(payload)
            .send(guild_id, &self.socket_write()?)
            .await?;

        Ok(())
    }

    /// Alias to `seek()`
    pub async fn jump_to_time(
        &self,
        guild_id: impl Into<GuildId> + Send,
        time: Duration,
    ) -> LavalinkResult<()> {
        self.seek(guild_id, time).await
    }

    /// Alias to `seek()`
    pub async fn scrub(
        &self,
        guild_id: impl Into<GuildId> + Send,
        time: Duration,
    ) -> LavalinkResult<()> {
        self.seek(guild_id, time).await
    }

    /// Sets the volume of the player.
    pub async fn volume(
        &self,
        guild_id: impl Into<GuildId> + Send,
        volume: u16,
    ) -> LavalinkResult<()> {
        let good_volume = max(min(volume, 1000), 0);

        let payload = crate::model::Volume {
            volume: good_volume,
        };

        crate::model::SendOpcode::Volume(payload)
            .send(guild_id, &self.socket_write()?)
            .await?;

        Ok(())
    }

    /// Sets all equalizer levels.
    ///
    /// - There are 15 bands (0-14) that can be changed.
    /// - The floating point value is the multiplier for the given band.
    /// - The default value is 0.
    /// - Valid values range from -0.25 to 1.0, where -0.25 means the given band is completely muted, and 0.25 means it is doubled.
    /// - Modifying the gain could also change the volume of the output.
    pub async fn equalize_all(
        &self,
        guild_id: impl Into<GuildId> + Send,
        bands: [f64; 15],
    ) -> LavalinkResult<()> {
        let bands = bands
            .iter()
            .enumerate()
            .map(|(index, i)| crate::model::Band {
                band: index as u8,
                gain: *i,
            })
            .collect::<Vec<_>>();

        let payload = crate::model::Equalizer { bands };

        crate::model::SendOpcode::Equalizer(payload)
            .send(guild_id, &self.socket_write()?)
            .await?;

        Ok(())
    }

    /// Equalize a dynamic set of bands, rather than just one or all of them at once.
    ///
    /// Unmentioned bands will remain unmodified.
    pub async fn equalize_dynamic(
        &self,
        guild_id: impl Into<GuildId> + Send,
        bands: Vec<Band>,
    ) -> LavalinkResult<()> {
        let payload = crate::model::Equalizer { bands };

        crate::model::SendOpcode::Equalizer(payload)
            .send(guild_id, &self.socket_write()?)
            .await?;

        Ok(())
    }

    /// Equalizes a specific band.
    pub async fn equalize_band(
        &self,
        guild_id: impl Into<GuildId> + Send,
        band: crate::model::Band,
    ) -> LavalinkResult<()> {
        let payload = crate::model::Equalizer { bands: vec![band] };

        crate::model::SendOpcode::Equalizer(payload)
            .send(guild_id, &self.socket_write()?)
            .await?;

        Ok(())
    }

    /// Resets all equalizer levels.
    pub async fn equalize_reset(&self, guild_id: impl Into<GuildId> + Send) -> LavalinkResult<()> {
        let bands = (0..=14)
            .map(|i| crate::model::Band {
                band: i as u8,
                gain: 0.,
            })
            .collect::<Vec<_>>();

        let payload = crate::model::Equalizer { bands };

        crate::model::SendOpcode::Equalizer(payload)
            .send(guild_id, &self.socket_write()?)
            .await?;

        Ok(())
    }

    /// Gets the discord gateway data.
    #[cfg(feature = "discord-gateway")]
    #[must_use]
    pub fn discord_gateway_data(&self) -> DiscordGatewayData {
        self.inner.lock().discord_gateway_data.clone()
    }

    /// Gets the list of voice connections from the discord gateway.
    #[cfg(feature = "discord-gateway")]
    #[must_use]
    pub fn discord_gateway_connections(&self) -> Arc<DashMap<GuildId, ConnectionInfo>> {
        self.inner.lock().discord_gateway_data.connections.clone()
    }

    #[cfg(feature = "discord-gateway")]
    /// Joins the voice channel via the discord gateway.
    pub async fn join(
        &self,
        guild_id: impl Into<GuildId> + Send,
        channel_id: impl Into<ChannelId> + Send,
    ) -> LavalinkResult<ConnectionInfo> {
        crate::voice::join(self, guild_id, channel_id).await
    }

    #[cfg(feature = "discord-gateway")]
    /// Joins the voice channel from the guild using the discord gateway.
    pub async fn leave(&self, guild_id: impl Into<GuildId> + Send) -> LavalinkResult<()> {
        crate::voice::leave(self, guild_id).await
    }
}
