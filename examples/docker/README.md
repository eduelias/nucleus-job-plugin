# End-to-end demo (Docker + a real Greengrass nucleus)

Try `nucleus-job-plugin` end to end with **only your AWS credentials**. This spins up the official
AWS IoT Greengrass nucleus in Docker (auto-provisioning the thing, thing group, and IAM role),
deploys the plugin as a **local component** via the in-container Greengrass CLI, then creates an
`AWS-Run-Command` IoT Job and shows it reach **`SUCCEEDED`**.

> This is exactly the workflow people keep asking about, e.g.
> [Managing IoT Core Jobs with Greengrass v2](https://repost.aws/questions/QUuwe0aQqZT2q7-h3YDNaTlg),
> [How to send iot jobs like Amazon managed restart template to a Greengrass Core device](https://repost.aws/questions/QUMwi9PxDcRT6xu1jEQEfj8g),
> and [How do I run an IoT Job on a GG v2 core?](https://repost.aws/questions/QUFtukwwjrT_C3xlEEqiqC8Q)

## ⚠️ This creates billable AWS resources

The demo creates an **IoT thing**, **thing group**, an **IAM token-exchange role + alias**, exchanges
**MQTT messages**, and creates an **IoT Job execution**. Costs are small but non-zero. **Always run
[`./teardown.sh`](teardown.sh)** when finished.

## Requirements

- Docker (Engine 20+) with the Compose plugin, able to run **`linux/amd64`** images.
  On Apple Silicon / ARM hosts this runs under emulation — it works but is slower.
- AWS CLI v2, `git`, `curl`.
- An IAM identity allowed to provision Greengrass resources. See the AWS
  [minimal IAM policy for the installer](https://docs.aws.amazon.com/greengrass/v2/developerguide/provision-minimal-iam-policy.html).
- A prebuilt release: the demo downloads the **x86_64** artifact from the
  [nucleus-job-plugin releases](https://github.com/eduelias/nucleus-job-plugin/releases)
  (override the version with `PLUGIN_VERSION=0.1.x`).

## Quickstart

```bash
cd examples/docker

# 1. Configure
cp .env.example .env                                   # set AWS_REGION
cp greengrass-v2-credentials/credentials.example \
   greengrass-v2-credentials/credentials               # add your AWS keys

# 2. Run (builds the GG image on first run, provisions, deploys, runs a job)
./run-demo.sh

# 3. Clean up (deletes the container + the AWS resources it created)
./teardown.sh
rm -f greengrass-v2-credentials/credentials
```

Expected tail:

```
[demo] job status: IN_PROGRESS
[demo] job status: SUCCEEDED
[demo] ✅ SUCCESS — job njp-demo-runcmd-... reached SUCCEEDED.
... marker file created by the handler
```

## What the demo does

1. Builds the Greengrass nucleus image from the official
   [`aws-greengrass/aws-greengrass-docker`](https://github.com/aws-greengrass/aws-greengrass-docker)
   Dockerfile (AWS doesn't publish a public image).
2. `docker compose up` with `PROVISION=true` — the installer creates the thing, thing group, IAM
   role/alias, and starts the nucleus. Ephemeral names get a random suffix (`njp-demo-xxxxxx`).
3. Downloads the plugin's x86_64 release, lays out a local `recipeDir` + `artifactDir`, and runs
   `greengrass-cli deployment create --merge dev.du7.nucleus-job-plugin=<version>` inside the
   container — a **local** deployment, no S3 and no cloud component version.
4. Waits for the component to reach `RUNNING`.
5. Creates an IoT Job from the **`AWS-Run-Command`** managed template (`touch /tmp/njp-demo-ok` — a
   safe no-op) targeting the demo thing, and polls until the execution is `SUCCEEDED`.

The plugin reaches AWS IoT Core through **Greengrass IPC** (`transport=ipc`), reusing the nucleus's
own MQTT connection — no extra certificate, no client-ID clash with the nucleus.

## Configuration

| Variable | Where | Meaning |
|---|---|---|
| `AWS_REGION` | `.env` | region to provision in / target |
| `THING_NAME` / `THING_GROUP_NAME` | `.env` | `*-REPLACE` gets a random suffix at runtime |
| `PLUGIN_VERSION` | env for `run-demo.sh` | release version to download (default `0.1.2`) |
| `DEPLOY_DEV_TOOLS` | `.env` | `true` installs the Greengrass CLI (needed for local deploy) |

## Production-like alternative (S3 + cloud component)

The demo uses a **local** deployment for simplicity. To deploy the way you would in production —
publish a cloud component version and target a thing/thing group — use the GDK + `deploy.sh` flow
documented in the [main README](../../README.md#deploying-as-a-greengrass-component):

```bash
gdk component build
gdk component publish --bucket <your-artifact-bucket>
greengrass/deploy.sh <version> <thing-name>
```

## Troubleshooting

- **Core device never HEALTHY** → `docker logs njp-greengrass-demo`. Usually credentials/region or
  the installer IAM permissions.
- **Download 404** → no `v$PLUGIN_VERSION` release with an x86_64 asset yet; set `PLUGIN_VERSION` to
  an existing release.
- **Job stays QUEUED** → confirm the component is `RUNNING`
  (`docker exec njp-greengrass-demo /greengrass/v2/bin/greengrass-cli component list`) and check
  `/greengrass/v2/logs/dev.du7.nucleus-job-plugin.log`.
- **Port 8883 in use** → stop other MQTT clients or edit the `ports` mapping in `docker-compose.yml`.

> Unofficial — not affiliated with, endorsed by, or sponsored by Amazon.
