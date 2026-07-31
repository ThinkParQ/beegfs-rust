# Flows

This is how some of the control flows might roughly work in the new auth system, without
incorporating gRPC on the servers or clients. The UDP node registration/"heartbeat" is gone.
We ignore the rest of the UDP messages, which mostly are short notifications that tell the node
to pull some new state. They might as well stay insecure for now.

## New cluster / new management

* Run `--init`, initializing management.
* Auto generate a management identity and store the keys on disk (alternatively let the admin import a pregerated key pair).
  * Print the public key as it is needed for all other nodes config.
* Auto generate a ctl identity and print/store the private key once (alternatively let
  the admin import a pregenerated public key).

The ctl identity is needed for ctl to authenticate and communicate directly with server nodes via
BeeMsg.

## Server node first registration

Server nodes do no longer automatically register, they need preregistration with management (see
above).

* Run setup command on node, generating public-private key pair and store it on disk (or import a pregenerated one).
  Also import and store the management public key.
* Add/import the server node to management, using its public key. Define node id and alias or let
  auto choose.
  * Could provide a bulk import from a file to make it easier.
  * Optionally allow ctl registration at runtime, using a ctl identity.
* Run the node. The node is already preregistered and identified by its public key.
* Fetch the full node- and identity list from management, including identities not bound to nodes (e.g.
  for ctl). Include the nic lists. This needs to use the configured management address.
* Fetch the nodes own id and other necessary info (e.g. alias) from management using the public key
and store it on disk.

## Server node reregistration / normal start

* Run the node. The node is already registered and identified by its public key.
* Fetch the full node- and identity list from management, including identities not bound to nodes (e.g.
  for ctl). Include the nic lists. This needs to use the configured management address.
* Fetch the nodes id and other necessary info (e.g. alias) using the public key and verify that
  the stored node id matches the one management reports. Update alias.

## Identity list change

* Ctl issues a change in identities (e.g. add new node, remove identity, ...) OR the management
  (re-)starts
* Send an update notification including a increasing generation number. Nodes can then decide to pull the
  new list if outdated. It's idmempotent. Might use UDP while it remains.
* In case of revocations, existing connections could be force-terminated (or instead actions being
  denied at a later time during authorization check (not part of this plan))

I considered a direct push mechanism to be more direct, but the above approach makes more
sense as it avoids unnecessary updates and avoids double implementation.
In addition, a generation check (and potential update) can be triggered when a handshake with a
peer fails.
In addition, the nodes should periodically check the current generation during internodesyncer cycle to
make sure to get informed about revocations quickly.


## Run ctl command against server nodes

* Server nodes have the identity list and verify the public key ctl sends against the list.
* Then they do the normal handshake for the connection.



# Design questions

## How to store managements public key

Can the existing management certificate (currently for gRPC only) be used for storing/deriving
managements public key to avoid having to provide an extra file to all the nodes? It also needs to
work for the client which can't do x.509 in the kernel. Maybe this should be done with a userspace
component at mount.

-> Claude recommends using an extra file as an x509 server certificate uses rsa or ecdsa, and the
cert key might not be ed25519. And even if, it would be an antipattern.

