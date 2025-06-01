#!/bin/bash

# parlor-room deployment script
set -euo pipefail

# Script directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

# Default values
IMAGE_NAME="parlor-room"
IMAGE_TAG="latest"
REGISTRY=""
ENVIRONMENT="development"
BUILD_ARGS=""

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Logging functions
log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

log_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Usage information
usage() {
    cat << EOF
Usage: $0 [OPTIONS] COMMAND

Commands:
    build           Build the Docker image
    push            Push image to registry
    deploy          Deploy with docker-compose
    clean           Clean up local images and containers
    test            Run tests in container
    help            Show this help message

Options:
    -t, --tag TAG         Set image tag (default: latest)
    -r, --registry REG    Set registry URL
    -e, --env ENV         Set environment (development|staging|production)
    --build-arg ARG       Pass build argument to Docker
    --no-cache            Build without using cache

Examples:
    $0 build
    $0 build --tag v1.0.0 --no-cache
    $0 push --registry ghcr.io/username --tag v1.0.0
    $0 deploy --env production
    $0 clean

EOF
}

# Parse command line arguments
parse_args() {
    while [[ $# -gt 0 ]]; do
        case $1 in
            -t|--tag)
                IMAGE_TAG="$2"
                shift 2
                ;;
            -r|--registry)
                REGISTRY="$2"
                shift 2
                ;;
            -e|--env)
                ENVIRONMENT="$2"
                shift 2
                ;;
            --build-arg)
                BUILD_ARGS="$BUILD_ARGS --build-arg $2"
                shift 2
                ;;
            --no-cache)
                BUILD_ARGS="$BUILD_ARGS --no-cache"
                shift
                ;;
            -h|--help)
                usage
                exit 0
                ;;
            build|push|deploy|clean|test|help)
                COMMAND="$1"
                shift
                ;;
            *)
                log_error "Unknown option: $1"
                usage
                exit 1
                ;;
        esac
    done
}

# Build the Docker image
build_image() {
    log_info "Building parlor-room Docker image..."
    
    cd "$PROJECT_ROOT"
    
    # Construct full image name
    local full_image_name="$IMAGE_NAME:$IMAGE_TAG"
    if [[ -n "$REGISTRY" ]]; then
        full_image_name="$REGISTRY/$full_image_name"
    fi
    
    log_info "Image name: $full_image_name"
    log_info "Build args: $BUILD_ARGS"
    
    # Build the image
    docker build $BUILD_ARGS -t "$full_image_name" .
    
    # Also tag as latest if not already
    if [[ "$IMAGE_TAG" != "latest" ]]; then
        local latest_image_name="$IMAGE_NAME:latest"
        if [[ -n "$REGISTRY" ]]; then
            latest_image_name="$REGISTRY/$latest_image_name"
        fi
        docker tag "$full_image_name" "$latest_image_name"
        log_info "Also tagged as: $latest_image_name"
    fi
    
    log_success "Build completed successfully!"
    
    # Show image info
    docker images "$IMAGE_NAME" --format "table {{.Repository}}\t{{.Tag}}\t{{.Size}}\t{{.CreatedAt}}"
}

# Push image to registry
push_image() {
    if [[ -z "$REGISTRY" ]]; then
        log_error "Registry not specified. Use --registry option."
        exit 1
    fi
    
    local full_image_name="$REGISTRY/$IMAGE_NAME:$IMAGE_TAG"
    
    log_info "Pushing image to registry: $full_image_name"
    docker push "$full_image_name"
    
    if [[ "$IMAGE_TAG" != "latest" ]]; then
        local latest_image_name="$REGISTRY/$IMAGE_NAME:latest"
        docker push "$latest_image_name"
        log_info "Also pushed: $latest_image_name"
    fi
    
    log_success "Push completed successfully!"
}

# Deploy with docker-compose
deploy_service() {
    log_info "Deploying parlor-room service (environment: $ENVIRONMENT)..."
    
    cd "$PROJECT_ROOT"
    
    # Choose compose file based on environment
    local compose_file="docker-compose.yml"
    if [[ -f "docker-compose.$ENVIRONMENT.yml" ]]; then
        compose_file="docker-compose.$ENVIRONMENT.yml"
        log_info "Using environment-specific compose file: $compose_file"
    fi
    
    # Pull latest images and deploy
    docker compose -f "$compose_file" pull
    docker compose -f "$compose_file" up -d
    
    log_success "Deployment completed!"
    
    # Show running services
    log_info "Running services:"
    docker compose -f "$compose_file" ps
}

# Clean up local Docker resources
clean_resources() {
    log_info "Cleaning up Docker resources..."
    
    # Stop and remove containers
    log_info "Stopping containers..."
    docker compose down --remove-orphans || true
    
    # Remove images
    log_info "Removing images..."
    docker images "$IMAGE_NAME" -q | xargs -r docker rmi || true
    
    # Clean up dangling images and build cache
    log_info "Cleaning up build cache..."
    docker builder prune -f
    docker image prune -f
    
    log_success "Cleanup completed!"
}

# Run tests in container
test_container() {
    log_info "Running tests in container..."
    
    cd "$PROJECT_ROOT"
    
    # Build test image
    docker build --target builder -t "$IMAGE_NAME:test" .
    
    # Run tests
    docker run --rm "$IMAGE_NAME:test" cargo test --release
    
    log_success "Tests completed!"
}

# Main execution
main() {
    # Default command
    COMMAND="${1:-help}"
    
    if [[ $# -gt 0 ]]; then
        shift
    fi
    
    # Parse remaining arguments
    parse_args "$@"
    
    case "$COMMAND" in
        build)
            build_image
            ;;
        push)
            push_image
            ;;
        deploy)
            deploy_service
            ;;
        clean)
            clean_resources
            ;;
        test)
            test_container
            ;;
        help)
            usage
            ;;
        *)
            log_error "Unknown command: $COMMAND"
            usage
            exit 1
            ;;
    esac
}

# Check dependencies
check_dependencies() {
    local deps=("docker")
    local missing=()
    
    for dep in "${deps[@]}"; do
        if ! command -v "$dep" &> /dev/null; then
            missing+=("$dep")
        fi
    done
    
    # Check for docker compose plugin
    if ! docker compose version &> /dev/null; then
        missing+=("docker-compose")
    fi
    
    if [[ ${#missing[@]} -gt 0 ]]; then
        log_error "Missing dependencies: ${missing[*]}"
        log_error "Please install the missing dependencies and try again."
        exit 1
    fi
}

# Run main function if script is executed directly
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    check_dependencies
    main "$@"
fi 