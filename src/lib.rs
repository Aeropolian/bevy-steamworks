#![deny(missing_docs)]

//! This crate provides a [Bevy](https://bevyengine.org/) plugin for integrating with
//! the Steamworks SDK.
//!
//! The underlying steamworks crate comes bundled with the redistributable dynamic
//! libraries a compatible version of the SDK. Currently it's v153a.
//!
//! ## Usage
//!
//! To add the plugin to your app, simply add the `SteamworksPlugin` to your
//! `App`. This will require the `AppId` provided to you by Valve for initialization.
//!
//! ```rust no_run
//! use bevy::prelude::*;
//! use bevy_steamworks::*;
//!
//! fn main() {
//!   // Use the demo Steam AppId for SpaceWar
//!   App::new()
//!       .add_plugins(DefaultPlugins)
//!       .add_plugins(SteamworksPlugin::new(AppId(480)))
//!       .run()
//! }
//! ```
//!
//! The plugin adds `steamworks::Client` as a Bevy ECS resource, which can be
//! accessed like any other resource in Bevy. The client implements `Send` and `Sync`
//! and can be used to make requests via the SDK from any of Bevy's threads. However,
//! any asynchronous callbacks from Steam will only run on the main thread.
//!
//! The plugin will automatically call [`SingleClient::run_callbacks`] on the Bevy
//! main thread every frame in [`First`], so there is no need to run it
//! manually.
//!
//! **NOTE**: If the plugin fails to initialize (i.e. `Client::init()` fails and
//! returns an error, an error wil lbe logged (via `bevy_log`), but it will not
//! panic. In this case, it may be necessary to use `Option<Res<Client>>` instead.
//!
//! All callbacks are forwarded as [`Events`] and can be listened to in the a
//! Bevy idiomatic way:
//!
//! ```rust no_run
//! use bevy::prelude::*;
//! use bevy_steamworks::*;
//!
//! fn steam_system(steam_client: Res<Client>) {
//!   for friend in steam_client.friends().get_friends(FriendFlags::IMMEDIATE) {
//!     println!("Friend: {:?} - {}({:?})", friend.id(), friend.name(), friend.state());
//!   }
//! }
//!
//! fn main() {
//!   // Use the demo Steam AppId for SpaceWar
//!   App::new()
//!       .add_plugins(DefaultPlugins)
//!       .add_plugins(SteamworksPlugin::new(AppId(480)))
//!       .add_systems(Startup, steam_system)
//!       .run()
//! }
//! ```

use std::{ops::Deref, sync::Arc};

use bevy_app::{App, First, Plugin};
use bevy_ecs::{
    event::EventWriter,
    prelude::Event,
    schedule::*,
    system::{NonSend, Res, Resource},
};
use bevy_utils::syncunsafecell::SyncUnsafeCell;
use steamworks::CallbackHandle;
// Reexport everything from steamworks except for the clients
pub use steamworks::{
    networking_messages, networking_sockets, networking_utils, restart_app_if_necessary, AccountId,
    AppIDs, AppId, Apps, AuthSessionError, AuthSessionTicketResponse, AuthSessionValidateError,
    AuthTicket, Callback, ChatMemberStateChange, ComparisonFilter, CreateQueryError,
    DistanceFilter, DownloadItemResult, FileType, FloatingGamepadTextInputDismissed,
    FloatingGamepadTextInputMode, Friend, FriendFlags, FriendGame, FriendState, Friends, GameId,
    GameLobbyJoinRequested, GamepadTextInputDismissed, GamepadTextInputLineMode,
    GamepadTextInputMode, Input, InstallInfo, InvalidErrorCode, ItemState, Leaderboard,
    LeaderboardDataRequest, LeaderboardDisplayType, LeaderboardEntry, LeaderboardScoreUploaded,
    LeaderboardSortMethod, LobbyChatUpdate, LobbyDataUpdate, LobbyId, LobbyKey,
    LobbyKeyTooLongError, LobbyListFilter, LobbyType, Manager, Matchmaking,
    MicroTxnAuthorizationResponse, NearFilter, NearFilters, Networking, NotificationPosition,
    NumberFilter, NumberFilters, OverlayToStoreFlag, P2PSessionConnectFail, P2PSessionRequest,
    PersonaChange, PersonaStateChange, PublishedFileId, PublishedFileVisibility, QueryHandle,
    QueryResult, QueryResults, RemotePlay, RemotePlayConnected, RemotePlayDisconnected,
    RemotePlaySession, RemotePlaySessionId, RemoteStorage, SIResult, SResult, SendType, Server,
    ServerManager, ServerMode, SingleClient, SteamAPIInitError, SteamDeviceFormFactor, SteamError,
    SteamFile, SteamFileInfo, SteamFileReader, SteamFileWriter, SteamId, SteamServerConnectFailure,
    SteamServersConnected, SteamServersDisconnected, StringFilter, StringFilterKind, StringFilters,
    TicketForWebApiResponse, UGCContentDescriptorID, UGCQueryType, UGCStatisticType, UGCType,
    UpdateHandle, UpdateStatus, UpdateWatchHandle, UploadScoreMethod, User, UserAchievementStored,
    UserList, UserListOrder, UserRestriction, UserStats, UserStatsReceived, UserStatsStored, Utils,
    ValidateAuthTicketResponse, RESULTS_PER_PAGE, UGC,
};

#[derive(Resource)]
struct SteamEvents {
    _callbacks: Vec<CallbackHandle>,
    pending: Arc<SyncUnsafeCell<Vec<SteamworksEvent>>>,
}

/// A Bevy-compatible wrapper around various Steamworks events.
#[derive(Event)]
#[allow(missing_docs)]
pub enum SteamworksEvent {
    AuthSessionTicketResponse(steamworks::AuthSessionTicketResponse),
    DownloadItemResult(steamworks::DownloadItemResult),
    FakeIPResult(steamworks::networking_sockets::FakeIPResult),
    GameLobbyJoinRequested(steamworks::GameLobbyJoinRequested),
    GamepadTextInputDismissed(steamworks::GamepadTextInputDismissed),
    FloatingGamepadTextInputDismissed(steamworks::FloatingGamepadTextInputDismissed),
    LobbyChatUpdate(steamworks::LobbyChatUpdate),
    LobbyChatMsg(steamworks::LobbyChatMsg),
    LobbyDataUpdate(steamworks::LobbyDataUpdate),
    NetConnectionStatusChanged(steamworks::networking_types::NetConnectionStatusChanged),
    P2PSessionConnectFail(steamworks::P2PSessionConnectFail),
    P2PSessionRequest(steamworks::P2PSessionRequest),
    PersonaStateChange(steamworks::PersonaStateChange),
    RemotePlayConnected(steamworks::RemotePlayConnected),
    RemotePlayDisconnected(steamworks::RemotePlayDisconnected),
    SteamServerConnectFailure(steamworks::SteamServerConnectFailure),
    SteamServersConnected(steamworks::SteamServersConnected),
    SteamServersDisconnected(steamworks::SteamServersDisconnected),
    TicketForWebApiResponse(steamworks::TicketForWebApiResponse),
    MicroTxnAuthorizationResponse(steamworks::MicroTxnAuthorizationResponse),
    UserAchievementStored(steamworks::UserAchievementStored),
    UserStatsReceived(steamworks::UserStatsReceived),
    UserStatsStored(steamworks::UserStatsStored),
    ValidateAuthTicketResponse(steamworks::ValidateAuthTicketResponse),
}

macro_rules! register_event_callbacks {
    ($client: ident, $($event_name:ident: $event_ty:ty),+ $(,)?) => {
        {
            let pending = Arc::new(SyncUnsafeCell::new(Vec::new()));
            SteamEvents {
                _callbacks: vec![
                    $({
                        let pending_in = pending.clone();
                        $client.register_callback::<$event_ty, _>(move |evt| {
                            // SAFETY: The callback is only called during `run_steam_callbacks` which cannot run
                            // while any of the flush_events systems are running. This cannot alias.
                            unsafe {
                                (&mut *pending_in.get()).push(SteamworksEvent::$event_name(evt));
                            }
                        })
                    }),+
                ],
                pending,
            }
        }
    };
}

/// A Bevy compatible wrapper around [`steamworks::Client`].
///
/// Automatically dereferences to the client so it can be transparently
/// used.
///
/// For more information on how to use it, see [`steamworks::Client`].
#[derive(Resource, Clone)]
pub struct Client(steamworks::Client);

impl Deref for Client {
    type Target = steamworks::Client;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// A Bevy [`Plugin`] for adding support for the Steam SDK.
pub struct SteamworksPlugin(AppId);

impl SteamworksPlugin {
    /// Creates a new `SteamworksPlugin`. The provided `app_id` should correspond
    /// to the Steam app ID provided by Valve.
    pub fn new(app_id: impl Into<AppId>) -> Self {
        Self(app_id.into())
    }
}

impl Plugin for SteamworksPlugin {
    fn build(&self, app: &mut App) {
        if app.world.contains_resource::<Client>() {
            bevy_log::warn!("Attempted to add the Steamworks plugin multiple times!");
            return;
        }
        match steamworks::Client::init_app(self.0) {
            Err(err) => bevy_log::error!("Failed to initialize Steamworks client: {}", err),
            Ok((client, single)) => {
                app.insert_resource(Client(client.clone()))
                    .insert_resource(register_event_callbacks!(
                        client,
                        AuthSessionTicketResponse: steamworks::AuthSessionTicketResponse,
                        DownloadItemResult: steamworks::DownloadItemResult,
                        FakeIPResult: steamworks::networking_sockets::FakeIPResult,
                        GameLobbyJoinRequested: steamworks::GameLobbyJoinRequested,
                        GamepadTextInputDismissed: steamworks::GamepadTextInputDismissed,
                        FloatingGamepadTextInputDismissed: steamworks::FloatingGamepadTextInputDismissed,
                        LobbyChatUpdate: steamworks::LobbyChatUpdate,
                        LobbyChatMsg: steamworks::LobbyChatMsg,
                        LobbyDataUpdate: steamworks::LobbyDataUpdate,
                        NetConnectionStatusChanged: steamworks::networking_types::NetConnectionStatusChanged,
                        P2PSessionConnectFail: steamworks::P2PSessionConnectFail,
                        P2PSessionRequest: steamworks::P2PSessionRequest,
                        PersonaStateChange: steamworks::PersonaStateChange,
                        RemotePlayConnected: steamworks::RemotePlayConnected,
                        RemotePlayDisconnected: steamworks::RemotePlayDisconnected,
                        SteamServerConnectFailure: steamworks::SteamServerConnectFailure,
                        SteamServersConnected: steamworks::SteamServersConnected,
                        SteamServersDisconnected: steamworks::SteamServersDisconnected,
                        TicketForWebApiResponse: steamworks::TicketForWebApiResponse,
                        MicroTxnAuthorizationResponse: steamworks::MicroTxnAuthorizationResponse,
                        UserAchievementStored: steamworks::UserAchievementStored,
                        UserStatsReceived: steamworks::UserStatsReceived,
                        UserStatsStored: steamworks::UserStatsStored,
                        ValidateAuthTicketResponse: steamworks::ValidateAuthTicketResponse,
                    ))
                    .insert_non_send_resource(single)
                    .add_event::<SteamworksEvent>()
                    .configure_sets(First, SteamworksSystem::RunCallbacks)
                    .add_systems(
                        First,
                        run_steam_callbacks
                            .in_set(SteamworksSystem::RunCallbacks)
                            .before(bevy_ecs::event::EventUpdates),
                    );
            }
        }
    }
}

/// A set of [`SystemSet`]s for systems used by [`SteamworksPlugin`]
///
/// [`SystemSet`]: bevy_ecs::schedule::SystemSet
#[derive(Debug, Clone, Copy, Eq, Hash, SystemSet, PartialEq)]
pub enum SteamworksSystem {
    /// A system set that runs the Steam SDK callbacks. Anything dependent on
    /// Steam API results should scheduled after this. This runs in
    /// [`First`].
    RunCallbacks,
}

fn run_steam_callbacks(
    client: NonSend<SingleClient>,
    events: Res<SteamEvents>,
    mut output: EventWriter<SteamworksEvent>,
) {
    client.run_callbacks();
    // SAFETY: The callback is only called during `run_steam_callbacks` which cannot run
    // while any of the flush_events systems are running. The system is registered only once for
    // the client. This cannot alias.
    let pending = unsafe { &mut *events.pending.get() };
    if !pending.is_empty() {
        output.send_batch(pending.drain(0..));
    }
}
