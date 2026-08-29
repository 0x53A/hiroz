# Archived development patches

This directory preserves patches that are useful as design or incident records but are not
intended to be applied unchanged.

## `0001-Fix-rmw-graph-bootstrap-with-default-QoS.patch`

- Original commit: `4171acd` (`Fix rmw graph bootstrap with default QoS`)
- Based on: `c7bf42d87b08a82df35c9b716c85e5b0b6676781`
- Incident: endpoints using rmw_zenoh's omitted/system-default history depth were dropped while
  parsing graph liveliness tokens on ROS domain 123.

The patch is archived intact because it contains both the successful QoS parser fix and an
experimental asynchronous graph bootstrap. **Do not apply it as a whole.** Review found that the
graph portion can return an empty or partial graph before bootstrap begins, bypass graph-change
events for snapshot entities, hide initialization failures, truncate snapshots, and leave an
unowned background thread alive.

The production-safe follow-up keeps the QoS correction separately and restores the synchronous,
history-enabled graph construction. The original graph experiment remains here so its approach,
motivation, and failure modes are not lost.

For forensic use, inspect it with:

```console
git apply --stat docs/patches/0001-Fix-rmw-graph-bootstrap-with-default-QoS.patch
git apply --check docs/patches/0001-Fix-rmw-graph-bootstrap-with-default-QoS.patch
```

Only apply it to a disposable branch created from the recorded base commit.
