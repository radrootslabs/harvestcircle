#!/bin/sh
set -eu

repository_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
gradle="$repository_root/gradlew"
test_name='org.harvestcircle.buildlogic.plugins.ConventionPluginSmokeTest.packagingPluginRejectsMissingCloseTimeoutAndSecretOutput'
runs=25
run=1

while [ "$run" -le "$runs" ]; do
    printf '%s\n' "harvestcircle.build-logic-stability.run=$run/$runs"
    "$gradle" \
        --no-daemon \
        --no-configuration-cache \
        -p "$repository_root/build-logic" \
        :plugins:functionalTest \
        --tests "$test_name" \
        --rerun-tasks
    run=$((run + 1))
done

printf '%s\n' \
    'harvestcircle.build-logic-stability.prior-expected=did not report closed health evidence' \
    'harvestcircle.build-logic-stability.prior-observed=health-check timed out' \
    "harvestcircle.build-logic-stability.runs=$runs" \
    'harvestcircle.build-logic-stability.result=pass'
