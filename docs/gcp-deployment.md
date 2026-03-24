# GCP Deployment

Deploy the MetaStack API service to Google Cloud Run with Artifact Registry for container storage and Workload Identity Federation for keyless CI/CD.

## Architecture

```
GitHub Actions (CI/CD)
  │
  ├─ Build: multi-stage Docker build → static musl binary
  ├─ Push:  container image → Artifact Registry
  └─ Deploy: image → Cloud Run
```

Components provisioned by Terraform:

| Resource | Purpose |
|----------|---------|
| Artifact Registry | Stores container images (`metastack` repository) |
| Cloud Run v2 service | Runs the `metastack-api` container |
| Service account (runtime) | Identity the Cloud Run service runs as |
| Service account (deploy) | Identity GitHub Actions authenticates as |
| Workload Identity Federation | Allows GitHub Actions to authenticate without long-lived keys |

## Prerequisites

- [Terraform](https://developer.hashicorp.com/terraform/install) >= 1.5
- [gcloud CLI](https://cloud.google.com/sdk/docs/install) authenticated with a project-owner account
- A GCP project with billing enabled
- Docker (for local image builds)

## Initial setup

### 1. Provision GCP resources

```bash
cd deploy/terraform

# Provide your GCP project ID
terraform init
terraform plan -var="project_id=YOUR_PROJECT_ID"
terraform apply -var="project_id=YOUR_PROJECT_ID"
```

Save the outputs — you'll need them for GitHub repository configuration.

### 2. Configure GitHub repository variables

In **Settings > Secrets and variables > Actions > Variables**, add:

| Variable | Value |
|----------|-------|
| `GCP_PROJECT_ID` | Your GCP project ID |
| `GCP_REGION` | Region from Terraform (default: `us-central1`) |
| `GCP_WORKLOAD_IDENTITY_PROVIDER` | `workload_identity_provider` Terraform output |
| `GCP_SERVICE_ACCOUNT` | `github_actions_service_account` Terraform output |

### 3. Verify the deploy workflow

Push to `main` or trigger the workflow manually:

```bash
gh workflow run deploy
```

## Local image build

```bash
# Build
docker build -t metastack-api:local .

# Run
docker run --rm -p 8080:8080 metastack-api:local --help
```

## Manual deploy

Push an image and deploy without CI:

```bash
IMAGE="us-central1-docker.pkg.dev/YOUR_PROJECT_ID/metastack/metastack-api"
TAG="sha-$(git rev-parse --short HEAD)"

# Authenticate
gcloud auth configure-docker us-central1-docker.pkg.dev

# Build and push
docker build -t "${IMAGE}:${TAG}" .
docker push "${IMAGE}:${TAG}"

# Deploy
gcloud run deploy metastack-api \
  --region us-central1 \
  --image "${IMAGE}:${TAG}"
```

## Rollback

Deploy a previous image tag:

```bash
gcloud run deploy metastack-api \
  --region us-central1 \
  --image "us-central1-docker.pkg.dev/YOUR_PROJECT_ID/metastack/metastack-api:PREVIOUS_TAG"
```

Or use the Cloud Run console to select a previous revision.

## Environment variables

Pass runtime configuration to Cloud Run via the `env_vars` Terraform variable:

```hcl
env_vars = {
  LINEAR_API_KEY = "lin_api_..."
  LOG_LEVEL      = "info"
}
```

For secrets, prefer [Secret Manager](https://cloud.google.com/run/docs/configuring/services/secrets) integration over plain environment variables.

## Scaling

Adjust scaling in `deploy/terraform/variables.tf` or via overrides:

```bash
terraform apply \
  -var="project_id=YOUR_PROJECT_ID" \
  -var="min_instance_count=1" \
  -var="max_instance_count=10"
```

Setting `min_instance_count=1` keeps one instance warm to avoid cold starts.
