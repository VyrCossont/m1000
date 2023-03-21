# `m1000`

`m1000` is an automatic moderation tool ("automod") for Mastodon. It can match posts and accounts against rules based on words, regexes, or link domains, generate reports, and suspend or otherwise limit accounts that break the rules.

## Requirements

- A [Mastodon](https://github.com/mastodon/mastodon) instance running 4.1.x or higher (or the [Glitch](https://github.com/glitch-soc/mastodon) fork). Other forks may work if they have the `status.created` webhook.
- Administrator access to that instance, or at least the "Manage webhooks" and "Manage roles" permissions.
- A Linux or macOS machine to run `m1000` on. It doesn't need to be the same machine as any components of your Mastodon install, but it's best if it's 
- [A Rust development environment.](https://rustup.rs/) Prebuilt binaries and Docker containers are planned, but for now, you'll build `m1000` yourself.

### Optional dependencies

- [Rspamd](https://rspamd.com/) for trainable content filtering and access to third-party shared blocklists. Without Rspamd, you can still create your own rules.
- [Mastodon patch that extends the scope of `status.*` and `account.*` webhooks.](https://github.com/VyrCossont/mastodon/commit/df8be12f1190769aa530590163f9cbf56294dd52) Without this patch, `m1000` can only examine posts made by users on your instance.
- [Mastodon patch that adds a `report.updated` webhook.](https://github.com/VyrCossont/mastodon/commit/528afe989e1aa1e1a1069e2f420c498e78fc9f08) Without this patch, you can't capture training examples from closed reports. (However, this feature isn't implemented yet.)
- If you're running an `m1000` instance where webhook events need to travel across the public internet or another insecure network, you'll need a reverse proxy like [Nginx](https://www.nginx.com/) to handle TLS.

## Setup

You'll need to compile `m1000`, connect it to your instance's webhooks, set up a user account for it to use, and write some rules.

### Installing `m1000`

```bash
git clone https://github.com/VyrCossont/m1000.git
cd m1000
cargo build --release
```

The `m1000` binary is now in `target/release/m1000`.

### Configuring webhooks

1. Log into your Mastodon instance.
2. Go to `/admin/webhooks`.
3. Click "Add endpoint".
4. In the "Endpoint URL" box, enter the URL where your `m1000` instance will receive webhook events from Mastodon. In this example, we'll assume it's on the same machine as the Mastodon install, it's listening on its default port of 1337, and our instance is called `example.test`, so the endpoint URL will be `http://localhost:1337/webhook?domain=example.test`.
5. Under "Enabled events", check at least "status.created" and "status.updated". `m1000` doesn't use the other events yet, but future versions will.
6. Click "Add Endpoint".
7. Copy the "Signing secret", which should be a long hex string. You'll add this to `m1000`'s configuration later.

### Configuring a user account

First, create an account and give it the ability to suspend people. If you don't plan on using automatic suspensions, you can skip that part. (Any user can file a report; there are no special permissions for that.)

1. Create a new user account for `m1000` on your instance. In this example, we'll use the username `automod`.
   1. You can do this [on the command line](https://docs.joinmastodon.org/admin/tootctl/#accounts-create) from inside your Mastodon install directory: `RAILS_ENV=production bin/tootctl accounts create --email automod@example.test --confirmed --role Moderator --confirmed --skip-sign-in-token automod` will create the user `automod` with the `Moderator` role and a randomly generated password, which you should copy. The email account does not have to exist.
   2. If your instance has open registration, you can create a user account by going to `/auth/sign_up`, but seriously, why do you have open registration? Spammers love it.
2. Go to `/admin/accounts?origin=local`, find the `automod` user, and make sure it's confirmed and activated.
3. If you haven't already granted the `Moderator` role, or a custom role with the "Manage Users" permission, to the `automod` user, go to `/admin/roles` and do that now.

Next, set up the bot account. This is the same stuff you'd do for any bot account.

1. Open a new private browsing session.
2. Go to your instance and sign in with the `automod` user's email and password.
3. Go to `/settings/profile`. I recommend doing the following here:
   1. Check "Require follow requests", because there's no reason the automod account needs to have followers.
   2. Check "This is a bot account".
   3. Uncheck "Suggest account to others".
   4. Check "Hide your social graph".
   5. Set a spiffy display name, bio, header pic, and avatar pic.
   6. Click "Save changes".
4. Go to `/settings/preferences/other`.
   1. Check "Opt-out of search engine indexing" so your bot's profile doesn't show up in Google.
   2. Click "Save changes".
5. Go to `/settings/otp_authentication` and set up two-factor auth. This is a privileged account and you don't want it compromised.

### Configuring `m1000` proper

1. In the `m1000` checkout directory, run `target/release/m1000 --config-dir config setup --domain example.test --username automod` to start the interactive setup process. This will store your configuration in the `config` directory.
2. First, `m1000 setup` will prompt you for the webhook secret from when you set up the webhook.
3. Second, it will generate an authorization URL. Copy that and paste it into the private browsing session in which you are logged in as the `automod` user.
4. Grant it all the permissions it asks for.
   1. (`m1000` doesn't need all of these yet, so if you're being extra cautious, you can edit the `m1000` source to request only the ability to create reports and possibly suspend users. Future versions of `m1000` may need more permissions.)
5. Copy the authorization code and paste it in when `m1000 setup` asks for it.
6. When `m1000 setup` finishes, you'll find configuration YAML files in `config`.

### Running `m1000`

1. In the `m1000` checkout directory, run `target/release/m1000 --config-dir config serve`.
2. This will die when you log out. `m1000` doesn't have a `systemd` service file yet. but you can always run it under `tmux`.

## Configuration files

As above, we'll assume that the configuration files live in the `config` directory, and you have one instance named `example.test` with one bot user named `automod`.

### `config/global.yaml`

This file stores settings not related to any specific instance.

You can change the addresses and ports `m1000` listens on here. Note that `m1000 healthcheck` always uses the first one in the list.

You can also change the detected path to `rspamc` or delete the `rspamc_command` section entirely if you don't plan on using `rspamd`. `rspamc_command` takes a list of a command and its arguments, so you can use `['ssh', 'rspamd.example.test', 'rspamc']` here to talk to a remote `rspamd` instance, or one running in a Docker container, or something like that.

```yaml
listen:
- '[::]:1337'
rspamc_command:
- /usr/bin/rspamc
```

### `config/example.test/`

This directory contains all config for the `example.test` instance. `m1000` supports multiple instances.

### `config/example.test/app.yaml`

All Mastodon client apps [register to create an OAuth application on demand](https://docs.joinmastodon.org/client/token/) the first time they're used with a given instance. The resulting client ID and secret are stored here. Protect this file: don't share it and don't put it in Git. The scopes that the app is authorized for are also recorded here.

### `config/example.test/webhook.yaml`

This file stores the webhook secret that `m1000` uses to verify that incoming webhook events that claim to be from `example.test` are actually from `example.test`. Protect this file.

### `config/example.test/automod/`

This directory contains all configuration related to `example.test`'s `automod` user. `m1000` supports multiple user accounts per instance, which might be useful if you want different kinds of report to come from different bot users or something like that.

### `config/example.test/automod/credentials.yaml`

This file stores the OAuth access token that `m1000` uses to call Mastodon API methods as the `automod` user. Protect this file.

### `config/example.test/automod/config.yaml`

This file stores the rules that `m1000` judges incoming posts by. The default configuration reports any posts that link to Hacker News. You'll probably want to change that, or at least add some more rules.

```yaml
domain: demon.social
username: automod
rules:
- name: no orange website
  report:
    spam: false
    forward: false
  patterns:
  - post:
      text:
        link:
          domain: news.ycombinator.com
```

Each rule has a `name`, which is used when generating the text of a report to list all the rules that were broken by a given post.

Rules may have a `report` section. If this exists, the rule will create a report when it triggers:

- `forward` controls whether the report will be forwarded to a remote server if it's for a remote post. You probably want to start with forwarding off until you're sure of your rule and sure that it's something that the servers you might be reporting to will actually care about.
- `rule_ids`, if present, should be a list of instance rule IDs that will show up in the report. You can find your rule IDs by going to `/admin/rules`. Note that they have numbers in the UI, but those are not necessarily the actual rule IDs used by the Mastodon API. To find a rule ID, click on a given rule in the admin UI and note the URL. For example, rule #6 on my instance has the URL `/admin/rules/8/edit`, and thus ID `8`. Like all Mastodon API IDs, while they may be numbers, rule IDs must be treated as strings, so you'd write that as `rule_ids: ['8']` in `config.yaml`.
- `spam` may be set to `true` to report a post as spam when it triggers this rule. If `rule_ids` is present, `spam` will be ignored, and may be omitted.

Rules may also have a `restrict` section, which applies a [moderation action](https://docs.joinmastodon.org/admin/moderation/). `restrict` can be one of:
- [`sensitive`](https://docs.joinmastodon.org/admin/moderation/#sensitive-user): marks all of the user's media as [sensitive content](https://docs.joinmastodon.org/user/posting/#cw). Reversible.
- [`disable`](https://docs.joinmastodon.org/admin/moderation/#freeze-user): locks the user out of their account (also known as "freezing") but doesn't remove their profile or posts. Only works on users local to your instance. Reversible.
- [`silence`](https://docs.joinmastodon.org/admin/moderation/#limit-user): hides the account from all users on your instance (also known as "limiting"). Users who follow it can still see its posts, and they'll still show up in search, but not elsewhere. Reversible.
- [`suspend`](https://docs.joinmastodon.org/admin/moderation/#suspend-user): deletes an account from your instance. Reversible for up to 30 days, then the data is purged. Admins can force-delete the data early; `m1000` does not yet have that ability.

If both a `report` and a `restrict` section are present, the moderation action applied by `restrict` will automatically cite and close the report created by `report`, which is useful for maintaining an audit trail, and in the future, for creating filter training sets.

The `patterns` section contains the actual words, regexes, domains, etc. that the rule matches against. There are several different contexts where a match can be made, from post text to usernames. If multiple patterns are present, matching any pattern will trigger the rule. These need better documentation; see [`config.rs`](src/config.rs) from `RulePattern` down for the syntax.

## TODO

- a lot more pattern examples
- Rspamd configuration
- `systemd` service file
- prebuilt binaries and Docker images
