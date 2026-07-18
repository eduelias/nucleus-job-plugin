# Sample job handlers

These shell scripts are the **AWS IoT Device Client sample job handlers**, copied unmodified from
[`awslabs/aws-iot-device-client`](https://github.com/awslabs/aws-iot-device-client/tree/main/sample-job-handlers)
(licensed **Apache-2.0**). They implement the device side of the AWS **managed job templates**:

| Template | Handler |
|---|---|
| `AWS-Download-File` | `download-file.sh` |
| `AWS-Install-Application` | `install-packages.sh` |
| `AWS-Remove-Application` | `remove-packages.sh` |
| `AWS-Start-Application` | `start-services.sh` |
| `AWS-Stop-Application` | `stop-services.sh` |
| `AWS-Restart-Application` | `restart-services.sh` |
| `AWS-Reboot` | `reboot.sh` |

(`AWS-Run-Command` uses the `runCommand` action, not a handler script.)

## Invocation convention

The runner invokes these device-client style: the **first argument is the `runAsUser`** name (an
empty string when the job doesn't set one), followed by the template's own arguments. Each script is
responsible for dropping privileges itself (e.g. `sudo -u "$user"`), matching the device client's
behavior.

## Attribution / license

These files are third-party software under the Apache License 2.0. See `NOTICE` in the component root
for attribution. They are redistributed unmodified; upstream is the AWS IoT Device Client project.
