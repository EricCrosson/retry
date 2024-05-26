# retry

[![Build Status]](https://github.com/EricCrosson/retry/actions/workflows/release.yml)

[build status]: https://github.com/EricCrosson/retry/actions/workflows/release.yml/badge.svg?event=push

**retry** helps you retry a shell command until it succeeds.
It was written to make one-liners as readable and intuitive as possible.

## Use

The "big idea" of **retry** is that it eliminates ambiguous inputs by accepting either the number of times to try a command or the total length of time to spend (re)trying it.
This information is expressed by the required `--up-to` argument:

```bash
retry --up-to 5x npm install  # Retry for up to 5 times "npm install"
retry --up-to 10s npm install # Retry for up to 10 seconds "npm install"
```

To constrain the total runtime of an individual attempt, use `--task-timeout`:

```bash
retry --up-to 10m --task-timeout 15s -- zhu-li --do the-thing
```

## Optional arguments

| Argument name    | Description                                                                                        |
| ---------------- | -------------------------------------------------------------------------------------------------- |
| `--every`        | A constant duration to wait between attempts.                                                      |
| `--on-exit-code` | Only retry when the specified command failed with this exit code. Can be specified multiple times. |

### Argument formats

Durations are specified as `[0-9]+(ns|us|ms|[smhdwy])`.

`--up-to` accepts either a duration or the number of times attempts, specified as `[0-9]+x`.

## Acknowledgements

This command is heavily inspired by [joshdk/retry], which supports way more features!

Thanks for showing the utility of a statically-compiled `retry` command :bow:

[joshdk/retry]: https://github.com/joshdk/retry/
