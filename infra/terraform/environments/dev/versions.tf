terraform {
  required_version = ">= 1.10.0, < 2.0.0"

  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 6.72"
    }
  }

  # Partial configuration: the bucket comes from ../../backend.hcl
  # (`terraform init -backend-config=../../backend.hcl`).
  backend "s3" {
    key          = "einkaufio/dev/terraform.tfstate"
    region       = "eu-central-1"
    encrypt      = true
    use_lockfile = true
  }
}

provider "aws" {
  region = var.region

  default_tags {
    tags = {
      Project     = "einkaufio"
      ManagedBy   = "terraform"
      Environment = "dev"
    }
  }
}
