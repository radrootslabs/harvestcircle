use super::*;
use harvestcircle_application::hub_submission::HubSubmittedMaterial;

#[tokio::test]
async fn count_inventory_encoding_is_explicit_and_keeps_physical_material_unchanged() {
    let (fixture, db, command) = actual_world().await;
    db.host()
        .transaction(|tx| {
            Box::pin(async move {
                let hub = reference::authorize_on(tx, owner()).await?.id();
                let mut context = super::super::load_on(tx, hub, &command, None).await?;
                let own = crate::hub::actuals::common::operation_id(&operation(1900))?;
                assert!(context.count_aware);
                let current = context.submitted_material(own)?;
                let return_scope = context
                    .return_scope
                    .take()
                    .expect("native22 scope even when empty");
                context.count_aware = false;
                let old = context.submitted_material(own)?;
                assert_eq!(old.len(), 1);
                let old_bytes = serde_json::to_vec(&old).unwrap();
                // Isolated encoding probe: neither branch selects a native mode
                // or authenticates an original. Current22 retains both extensions.
                context.count_aware = true;
                let encoded = context.submitted_material(own)?;
                assert_eq!(encoded.len(), 2);
                assert_eq!(serde_json::to_vec(&encoded[..1]).unwrap(), old_bytes);
                let HubSubmittedMaterial::KnownCountInventory(inventory) = &encoded[1] else {
                    panic!("inventory")
                };
                assert_eq!(inventory.id, own);
                assert_eq!(
                    inventory.inventory.facts().records.len(),
                    context.known.len()
                );
                let value = serde_json::to_value(inventory).unwrap();
                assert_eq!(value["inventory"]["schema"], 2);
                assert_eq!(
                    value["inventory"]["count_accounts"]
                        .as_array()
                        .unwrap()
                        .len(),
                    0
                );
                context.return_scope = Some(return_scope);
                assert_eq!(context.submitted_material(own)?, current);
                context.return_scope = None;
                context.known.clear();
                let empty = context.submitted_material(own)?;
                let HubSubmittedMaterial::KnownCountInventory(inventory) = &empty[1] else {
                    panic!("empty inventory")
                };
                assert!(inventory.inventory.facts().records.is_empty());
                Ok::<_, SafeError>(())
            })
        })
        .await
        .unwrap();
    db.close().await.unwrap();
    drop(fixture);
}
