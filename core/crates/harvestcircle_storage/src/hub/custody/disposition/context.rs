use super::*;
use harvestcircle_domain::hub::movement::PhysicalStockGroup;
use harvestcircle_domain::hub::portions::PhysicalLineage;
use harvestcircle_domain::hub::treatment::TreatmentState;
pub(in crate::hub) struct Context {
    pub state: stock::read::StockState,
    pub catalog: ReferenceCatalog,
    pub group: PhysicalStockGroup,
    pub lineage: PhysicalLineage,
    pub treatment: TreatmentState,
    pub activity: DispositionActivity,
}
impl Context {
    pub(in crate::hub) fn domain(&self, hub: HubId) -> DispositionContext<'_> {
        DispositionContext {
            hub,
            stock_revision: self.state.revision,
            catalog: &self.catalog,
            group: &self.group,
            lineage: &self.lineage,
            treatment: &self.treatment,
            activity: self.activity,
        }
    }
}
pub(in crate::hub) async fn load_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    operation: &DurableRequestId,
    command: &DispositionCommand,
) -> Result<Context, SafeError> {
    let state = stock::read::state_on(tx, hub).await?;
    let catalog = reference::load_catalog_on(tx, hub).await?;
    if command.expected_stock_revision != state.revision
        || command.expected_catalog_revision != catalog.revision()
    {
        return Err(stock::conflict());
    }
    let group = handling::read::group_on(tx, hub, command.group, &catalog).await?;
    let loaded = handling::lineage::load_on(tx, hub, group.trace, &catalog, state.revision).await?;
    let treatment = handling::treatment::read::state_on(tx, hub, &loaded.graph).await?;
    let facts = crate::hub::submission::uncertainty_on(tx, operation).await?;
    Ok(Context {
        state,
        catalog,
        group,
        lineage: loaded.graph,
        treatment,
        activity: DispositionActivity {
            uncertain_submissions: facts.uncertain_intent,
            unresolved_actuals: facts.unresolved_actual,
        },
    })
}
