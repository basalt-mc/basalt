//! Tests for ServerContext.

use std::sync::Arc;

use crate::broadcast::BroadcastMessage;
use crate::components::Rotation;
use crate::context::Context;
use crate::gamemode::Gamemode;
use crate::player::PlayerInfo;
use basalt_types::Uuid;

use super::ServerContext;
use super::response::Response;

fn test_world() -> Arc<basalt_world::World> {
    Arc::new(basalt_world::World::new_memory(42))
}

fn test_ctx() -> ServerContext {
    ServerContext::new(
        test_world(),
        PlayerInfo {
            uuid: Uuid::default(),
            entity_id: 1,
            username: "Steve".into(),
            rotation: Rotation {
                yaw: 0.0,
                pitch: 0.0,
            },
            position: crate::components::Position {
                x: 0.0,
                y: 64.0,
                z: 0.0,
            },
        },
    )
}

#[test]
fn player_identity() {
    let ctx = test_ctx();
    assert_eq!(ctx.player().uuid(), Uuid::default());
    assert_eq!(ctx.player().entity_id(), 1);
    assert_eq!(ctx.player().username(), "Steve");
}

#[test]
fn send_message_queues_response() {
    let ctx = test_ctx();
    ctx.chat().send("hello");
    let responses = ctx.drain_responses();
    assert_eq!(responses.len(), 1);
    assert!(matches!(
        responses[0],
        Response::SendSystemChat {
            action_bar: false,
            ..
        }
    ));
}

#[test]
fn teleport_queues_position() {
    let ctx = test_ctx();
    ctx.player().teleport(10.0, 64.0, -5.0, 90.0, 0.0);
    let responses = ctx.drain_responses();
    assert_eq!(responses.len(), 1);
    assert!(matches!(responses[0], Response::SendPosition { .. }));
}

#[test]
fn set_gamemode_queues_state_change() {
    let ctx = test_ctx();
    ctx.player().set_gamemode(Gamemode::Creative);
    let responses = ctx.drain_responses();
    assert_eq!(responses.len(), 1);
    assert!(matches!(responses[0], Response::SendGameStateChange { .. }));
}

#[test]
fn broadcast_message_queues_broadcast() {
    let ctx = test_ctx();
    ctx.chat().broadcast("hello all");
    let responses = ctx.drain_responses();
    assert_eq!(responses.len(), 1);
    assert!(matches!(
        responses[0],
        Response::Broadcast(BroadcastMessage::Chat { .. })
    ));
}

#[test]
fn drain_clears_queue() {
    let ctx = test_ctx();
    ctx.chat().send("a");
    ctx.chat().send("b");
    assert_eq!(ctx.drain_responses().len(), 2);
    assert!(ctx.drain_responses().is_empty());
}

#[test]
fn context_trait_is_usable_as_dyn() {
    let ctx = test_ctx();
    let dyn_ctx: &dyn Context = &ctx;
    dyn_ctx.chat().send("via trait");
    assert_eq!(ctx.drain_responses().len(), 1);
}

#[test]
fn recipes_unlock_queues_response() {
    use crate::context::UnlockReason;
    use basalt_recipes::RecipeId;

    let ctx = test_ctx();
    let id = RecipeId::vanilla("oak_planks");
    ctx.recipes().unlock(&id, UnlockReason::Manual);

    let responses = ctx.drain_responses();
    assert_eq!(responses.len(), 1);
    match &responses[0] {
        Response::UnlockRecipe { recipe_id, reason } => {
            assert_eq!(recipe_id, &id);
            assert_eq!(*reason, UnlockReason::Manual);
        }
        other => panic!("expected UnlockRecipe, got {other:?}"),
    }
}

#[test]
fn recipes_lock_queues_response() {
    use basalt_recipes::RecipeId;

    let ctx = test_ctx();
    let id = RecipeId::new("plugin", "obsolete");
    ctx.recipes().lock(&id);

    let responses = ctx.drain_responses();
    assert_eq!(responses.len(), 1);
    match &responses[0] {
        Response::LockRecipe { recipe_id } => {
            assert_eq!(recipe_id, &id);
        }
        other => panic!("expected LockRecipe, got {other:?}"),
    }
}

#[test]
fn recipes_has_reads_snapshot() {
    use crate::components::KnownRecipes;
    use basalt_recipes::RecipeId;

    let id = RecipeId::vanilla("oak_planks");
    let mut snapshot = KnownRecipes::default();
    snapshot.unlock(id.clone());

    let ctx = ServerContext::new(
        test_world(),
        PlayerInfo {
            uuid: Uuid::default(),
            entity_id: 1,
            username: "Steve".into(),
            rotation: Rotation {
                yaw: 0.0,
                pitch: 0.0,
            },
            position: crate::components::Position {
                x: 0.0,
                y: 64.0,
                z: 0.0,
            },
        },
    )
    .with_known_recipes(snapshot);

    assert!(ctx.recipes().has(&id));
    assert!(!ctx.recipes().has(&RecipeId::vanilla("missing")));
    let snapshot_ids = ctx.recipes().unlocked();
    assert_eq!(snapshot_ids, vec![id]);
}
