`spread-adhoc-allocator` is a flexible ad-hoc system allocator designed for use
with [spread](https://github.com/canonical/spread).

Currently, spread test nodes can only be allocated using
[LXD](https://github.com/canonical/lxd). The key differences between this system
and a native LXD backend supported by spread are its focus on VM mode and
greater flexibility in resource assignment.

Refer to the [spread-lxd.yaml](./tree/master/spread-lxd.yaml) file for an
example configuration of the LXD backend. Additionally, the
[spread.yaml](./tree/master/spread.yaml) file provides an example of integrating
`spread-adhoc-allocator` as a spread `adhoc` backend.

🚧 TODO:
 - [ ] integrate [image-garden](https://gitlab.com/zygoon/image-garden)
