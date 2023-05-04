Framework for running tests against BeeGFS daemons. Currently only for mgmtd.

# How to set up

Build the beegfs base docker image using the dockerfile in this repo. Make sure to set the name to `beegfs-base`:

```
$ docker build -t beegfs-base -f <PATH_TO_REPO>/net-test/docker/base/Dockerfile <PATH_TO_BEEGFS_REPO>
```

Build the mgmtd docker image using the dockerfile in this repo. Make sure to set the BeeGFS repo as the build context and the name to `beegfs-mgmtd`:

```
$ docker build -t beegfs-mgmtd -f <PATH_TO_REPO>/net-test/mgmtd/base/Dockerfile <PATH_TO_BEEGFS_REPO>
```

Now, the container should be able to run and config parameters can be passed in as they can directly to the mgmtd binary:

```
$ docker run --rm beegfs-mgmtd logLevel=3
```

The other daemons can be built the same way.

# How to run

Just use `cargo test` and tell the test harness to run in a single thread. Otherwise, they will most likely fail because only one container can currently be started and used at once. Also, there can be port conflicts between several tests run in parallel.

To run them one after another, invoke cargo like this:

```
$ cargo test -- --test-threads=1
```

or use the alias

```
$ cargo mgmtd-test
```