# retry

**retry** helps you retry a shell command until it succeeds.
It was written to make one-liners as readable and intuitive as possible.

##  Use

The "big idea" of **retry** is that it eliminates ambiguous inputs by accepting either the number of times to try a command or the total length of time to spend (re)trying it.

For example:

```bash
retry --up-to 5x npm install
retry --up-to 10s npm install
```

To constrain the total runtime of an individual attempt, use `--task-timeout`:

```bash
retry --up-to 10m --task-timeout 15s sh -c 'zhu-li --do the-thing'
```

### Argument formats

Durations are specified as `[0-9]+(ns|us|ms|[smhdwy])`.

`--up-to` accepts either a duration or the number of times attempts, specified as `[0-9]+x`.

## Acknowledgements

This command is heavily inspired by [joshdk/retry], which supports way more features!

Thanks for showing the utility of a statically-compiled `retry` command :bow:

[joshdk/retry]: https://github.com/joshdk/retry/
