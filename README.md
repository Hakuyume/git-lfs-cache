# git-lfs-cache

a custom git-lfs transfer agent with cache support

```console
$ git lfs-cache install --cache='{"filesystem": {"dir": "..."}}'
$ git lfs pull
$ git lfs-cache stats
```

### cache backends
- filesystem
- google_cloud_storage
- http
