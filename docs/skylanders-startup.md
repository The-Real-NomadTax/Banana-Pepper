# Skylanders Cloud Patrol

The recognized Unity 4.2.2 cabinet executable is SHA-256
`c9fa7f38ab8cc58ba3e2daaa806d07b87e730a9d49e505f0a1881882b99fc2e9`.
Select the complete DataPC folder, a supported flat/legacy library layout, or a ZAR.
The plan requires `Skylanders_Data/mainData`.

Native Windows and Linux Wine receive the shared mounted view. The adapter is
exposed in memory as PQMTClient.dll and KaboomComDll.dll, both beside the executable
and under Unity's Plugins directory. Unity resolution/fullscreen arguments and
keyboard/controller bindings come from the common settings API. The cabinet's
updater, SSH tunnel and reboot launch scripts are not executed.

The PQ Labs touch adapter follows the window and maps mouse/controller input to
touches. The Kaboom adapter handles coins and ticket payout. The Matrix dongle is
not emulated. The profile remains experimental; mounted gameplay needs validation.
Game writes, including Unity logs, persist in the per-user overlay. There is no
staged executable, asset mirror or `runtime/skylanders` tree.

`just build` discovers the compiled adapter through its Cargo metadata.
`arcade-desktop --action skylanders launch SOURCE` launches the registered plan.
