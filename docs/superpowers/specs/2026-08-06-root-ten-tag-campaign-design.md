# Root Ten-Tag Coverage Campaign Design

## Goal

Close ten pinned-ExifTool 13.59 `MISSING` coverage gaps with exactly one fresh
agent assigned to each unique bare tag name.

## Isolation and collision avoidance

Work occurs on branch `sweep/root-ten-tags-20260806` in
`.worktrees/root-ten-tag-campaign`. Before every assignment, the coordinator
checks process command lines and registered worktrees. A tag is replaced before
dispatch if its bare name appears in another process's arguments or another
campaign's reservation list.

The initial excluded names are `BatteryLevel`, `ComponentsConfiguration`,
`DustRemovalData`, `LensInfo`, `ImageWidth`, `PreviewImageWidth`, `SignType`,
`SourceImageWidth`, `ThermalData`, `PreviewImageStart`, `ThumbnailTIFF`,
`AntiFlicker`, `DarkFocusEnvironment`, `Author`, `AmbientTemperature`, `Azimuth`,
`SensorID`, `UniformResourceName`, and `TimeZoneOffset`.

## Assignments

1. APE `Duration` in `APE.ape`
2. BMP `Planes` in `BMP.bmp`
3. ICO `BitsPerPixel` in `ICO.ico`
4. ISO `VolumeSize` in `ISO.iso`
5. M4A `AvgBitrate` in `QuickTime.m4a`
6. MP3 `ID3Size` in `MP3.mp3`
7. PCAPNG `OperatingSystem` in `PCAP.pcapng`
8. RAM `URL` in `Real.ram`
9. WPG `Records` in `WPG.wpg`
10. DSS `EndTime` in `Olympus.dss`

All fixtures come from the pinned ExifTool tree's `t/images` corpus. The
baseline report is `/tmp/root-ten-tag-conformance.json`.

## Agent contract

Each fresh agent owns only its assigned tag. It must reproduce the gap with the
pinned oracle, check transcribed ExifTool tables before deriving layout, write
and observe a focused failing test, implement the smallest exact fix, verify the
focused and relevant tests, and commit only its tag. It must not emit adjacent
tags, approximate conversions, reset shared work, or substitute another tag.

Agents run sequentially in the shared integration worktree so commits and test
state remain deterministic. An unsuccessful agent still counts as the agent
assigned to that tag, but the campaign only claims fixes with passing evidence.

## Acceptance

Every accepted tag has pinned-oracle before/after evidence, a regression test,
and one independent commit. The final branch must pass formatting, relevant
Clippy and workspace tests, and a fresh conformance run covering the ten sample
files without introducing regressions.
