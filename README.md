# Automated Deployments API

This API contains just two endpoints:
  - `/api/ad`: a POST endpoint to receive webhooks from GitHub. (For information about the headers and payload of a GitHub webhook, see <https://docs.github.com/en/webhooks/webhook-events-and-payloads>.)
  - `/api/status`: a GET endpoint for monitoring.

When a webhook is successfully received by the API, it will attempt to redeploy the code by running an Ansible playbook on the <a href="https://github.com/dvrpc/cloud-ansible?tab=readme-ov-file#controller">controller</a>, using a tag that associates a set of tasks in the playbook with the repository the webhook is coming from. The results of the attempted deployment will be emailed to those in `EMAIL_RECEIVERS` (see below).

The program requires three environment variables in an ignored .env file:
  - `GITHUB_TOKEN`. This is kept as a secret in the "ad_api" role in the cloud-ansible project. (It also needs to be set in the repository the webhook is being called on - see below.)
  - `PATH_TO_ANSIBLE_PROJECT`. The path to the Ansible project on the controller (where the cloud-ansible repository is cloned to).
  - `EMAIL_RECEIVERS`. Those who should be emailed the results of an attempted redeployment, e.g. `EMAIL_RECEIVERS="Person1 <person1@dvrpc.org>,Person2 <person2@dvrpc.org>"`.

## How to Use the API for Deployments

1. Set up a webhook on the repository of the code to be deployed.
  a. go to Settings of the repository in GitHub
  b. select Webhooks
  c. click on the button "Add Webhook"
  d. Fill out the form:
    - use "https://controller.cloud.dvrpc.org/api/ad" for the payload url
    - select "application/json" for the content type
    - add secret mentioned above (GITHUB_TOKEN env var)
    - for "which events would you like to trigger with this webook", chose "Let me select individual events", click the checkbox for "Pull requests" and **uncheck** the default of "Pushes".
    - click the "Add webhook" button at the button to activate it.
2. In the code for this API (in src/main.rs), add the repo name/tag in the code and redeploy. (This is currently hard-coded, but probably shouldn't be.)

NOTE: upon setting up the webhook on a repository, it will attempt to ping the server. This will be reported as a failure, as we reject anything that's not a pull request with a certain body. It can be ignored.
