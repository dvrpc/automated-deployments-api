# Automated Deployments API

This API contains (at least for now) a single POST endpoint to receive Webhooks from GitHub. For the headers and payload of a GitHub Webhook, see <https://docs.github.com/en/webhooks/webhook-events-and-payloads>.

When a webhook is successfully received, the API will run an Ansible playbook on the server it is deployed on, using a tag that associates a set of tasks in the playbook with the repository the webhook is coming from, to redeploy that app/API.

The program requires two environment variables in an ignored .env file:
  - `GITHUB_TOKEN`. It should match the secret in the webhook configuration in each repository that we are setting up to have automated deployments.
  - `PATH_TO_ANSIBLE_PROJECT`. The path to the Ansible project on the host (where the cloud-ansible repository is cloned to).
