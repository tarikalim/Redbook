# Security policy

## Supported versions

Only the latest release gets fixes.

## Reporting a vulnerability

Please do not open a public issue. Report it privately through
[GitHub's private vulnerability reporting](https://github.com/tarikalim/Redbook/security/advisories/new),
or send a private message to [@tarikalim](https://github.com/tarikalim). Include the steps to
reproduce it and the version you used. You will get an answer within a week.

## Scope

Redbook runs locally: it keeps your notes in a SQLite file in your home folder and serves MCP over
stdio to the client that starts it. It opens no network port. The only network traffic is sync,
which runs `git` against the repository you configure, with your own git credentials.
