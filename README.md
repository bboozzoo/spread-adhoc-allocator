`spread-adhoc-allocator` is a flexible ad-hoc system allocator designed for use
with [spread](https://github.com/canonical/spread).

Currently, spread test nodes can only be allocated using
[LXD](https://github.com/canonical/lxd). The key differences between this system
and a native LXD backend supported by spread are its focus on VM mode and
greater flexibility in resource assignment.

Refer to the [spread-lxd.yaml](./spread-lxd.yaml) file for an
example configuration of the LXD backend. Additionally, the
[spread.yaml](./spread.yaml) file provides an example of integrating
`spread-adhoc-allocator` as a spread `adhoc` backend.

Quickly trying it out with the reference configuration:

```text
$ spread adhoc-lxd:ubuntu-24.04-64:examples/hello
2025-01-12 16:55:38 Project content is packed for delivery (19.59KB).
2025-01-12 16:55:38 If killed, discard servers with: spread -reuse-pid=69638 -discard
2025-01-12 16:55:38 Allocating adhoc-lxd:ubuntu-24.04-64...
2025-01-12 16:56:01 Waiting for adhoc-lxd:ubuntu-24.04-64 to make SSH available at 10.22.100.124:22...
2025-01-12 16:56:01 Allocated adhoc-lxd:ubuntu-24.04-64.
2025-01-12 16:56:01 Connecting to adhoc-lxd:ubuntu-24.04-64...
2025-01-12 16:56:02 Connected to adhoc-lxd:ubuntu-24.04-64 at 10.22.100.124:22.
2025-01-12 16:56:02 Sending project content to adhoc-lxd:ubuntu-24.04-64...
2025-01-12 16:56:02 Executing adhoc-lxd:ubuntu-24.04-64:examples/hello (adhoc-lxd:ubuntu-24.04-64) (1/1)...
2025-01-12 16:56:02 Discarding adhoc-lxd:ubuntu-24.04-64...
2025-01-12 16:56:03 Successful tasks: 1
2025-01-12 16:56:03 Aborted tasks: 0
```

Nodes can be allocated/deallocated manually, check out:

``` text
$ spread-adhoc-allocator allocate ubuntu-24.04-64 ubuntu ubuntu
10.22.100.124:22
$ spread-adhoc-allocator deallocate 10.22.100.124:22
$ spread-adhoc-allocator cleanup
```

Or explore `spread-adhoc-allocator help` for more details.

Due to a bug in spread where PATH is overwritten in `adhoc` backend allocator
snippets (fix in https://github.com/canonical/spread/pull/204), the
`spread-adhoc-allocator` binary must be made available under one of the standard
system locations (eg. /usr/local/bin). A temporary workaround until the upstream
fix is merged:

``` sh
sudo ln -s -v $PWD/target/debug/spread-adhoc-allocator /usr/local/bin/
```

🚧 TODO:
 - [ ] integrate [image-garden](https://gitlab.com/zygoon/image-garden)
 - [ ] support non VMs
