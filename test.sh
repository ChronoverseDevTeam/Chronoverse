#!/bin/bash
# Chronoverse 测试启动脚本 (Linux/Mac)

set -e

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
GRAY='\033[0;90m'
NC='\033[0m' # No Color

# 默认参数
TEST_TYPE="${1:-all}"
SHOW_OUTPUT=0
NO_CAPTURE=0
VERBOSE=0

# 解析参数
while [[ $# -gt 0 ]]; do
    case $1 in
        --show-output)
            SHOW_OUTPUT=1
            shift
            ;;
        --nocapture)
            NO_CAPTURE=1
            shift
            ;;
        --verbose)
            VERBOSE=1
            shift
            ;;
        -h|--help|help)
            show_help
            exit 0
            ;;
        *)
            TEST_TYPE="$1"
            shift
            ;;
    esac
done

# 显示帮助
show_help() {
    echo -e "${CYAN}用法: ./test.sh [测试类型] [选项]${NC}"
    echo ""
    echo -e "${YELLOW}测试类型:${NC}"
    echo "  all         运行所有测试 (默认)"
    echo "  grpc        运行 gRPC 集成测试"
    echo "  workspace   运行 workspace 测试"
    echo "  user        运行 user 测试"
    echo "  auth        运行认证测试"
    echo "  list        运行 list_workspaces 测试"
    echo "  unit        运行单元测试"
    echo ""
    echo -e "${YELLOW}选项:${NC}"
    echo "  --nocapture    不捕获测试输出 (显示 println!)"
    echo "  --show-output  显示成功测试的输出"
    echo "  --verbose      显示详细的错误信息（包括 backtrace）"
    echo ""
    echo -e "${YELLOW}示例:${NC}"
    echo -e "${GRAY}  ./test.sh                    # 运行所有测试${NC}"
    echo -e "${GRAY}  ./test.sh grpc               # 运行 gRPC 测试${NC}"
    echo -e "${GRAY}  ./test.sh grpc --verbose     # 运行 gRPC 测试并显示详细信息${NC}"
    echo -e "${GRAY}  ./test.sh workspace --show-output  # 运行 workspace 测试并显示输出${NC}"
}

# 检查 MongoDB
check_mongodb() {
    echo -e "${YELLOW}检查 MongoDB 连接...${NC}"
    
    if nc -z 127.0.0.1 27017 2>/dev/null || timeout 1 bash -c 'cat < /dev/null > /dev/tcp/127.0.0.1/27017' 2>/dev/null; then
        echo -e "${GREEN}✓ MongoDB 正在运行${NC}"
        return 0
    else
        echo -e "${RED}✗ MongoDB 未运行${NC}"
        echo ""
        echo -e "${YELLOW}请先启动 MongoDB:${NC}"
        echo "  cd crv-hive/mongo"
        echo "  docker-compose up -d"
        echo ""
        return 1
    fi
}

# 获取测试命令
get_test_command() {
    local type=$1
    local base_cmd="cargo test --package crv-hive --lib"
    local output_flags="-- --test-threads=1"
    
    if [ $SHOW_OUTPUT -eq 1 ]; then
        output_flags="-- --test-threads=1 --show-output"
    elif [ $NO_CAPTURE -eq 1 ]; then
        output_flags="-- --test-threads=1 --nocapture"
    fi
    
    case $type in
        all)
            echo "$base_cmd $output_flags"
            ;;
        grpc)
            echo "$base_cmd test_grpc $output_flags"
            ;;
        workspace)
            echo "$base_cmd workspace $output_flags"
            ;;
        user)
            echo "$base_cmd test_user $output_flags"
            ;;
        auth)
            echo "$base_cmd test_grpc_auth $output_flags"
            ;;
        list)
            echo "$base_cmd list_workspaces $output_flags"
            ;;
        unit)
            echo "$base_cmd --lib $output_flags"
            ;;
        *)
            echo -e "${RED}错误: 未知的测试类型 '$type'${NC}"
            show_help
            exit 1
            ;;
    esac
}

# 显示测试信息
show_test_info() {
    local type=$1
    
    echo -e "测试类型: ${GREEN}$type${NC}"
    echo ""
    
    case $type in
        all)
            echo -e "${GRAY}运行所有测试（包括 gRPC、workspace、user 等）${NC}"
            ;;
        grpc)
            echo -e "${GRAY}运行所有 gRPC 集成测试${NC}"
            echo -e "${GRAY}  - test_grpc_greeting${NC}"
            echo -e "${GRAY}  - test_grpc_auth_flow${NC}"
            echo -e "${GRAY}  - test_grpc_workspace_operations${NC}"
            echo -e "${GRAY}  - test_grpc_auth_validation${NC}"
            echo -e "${GRAY}  - test_grpc_workspace_validation${NC}"
            ;;
        workspace)
            echo -e "${GRAY}运行 workspace 相关测试${NC}"
            ;;
        user)
            echo -e "${GRAY}运行 user 相关测试${NC}"
            ;;
        auth)
            echo -e "${GRAY}运行认证相关测试${NC}"
            ;;
        list)
            echo -e "${GRAY}运行 list_workspaces 测试${NC}"
            ;;
    esac
    echo ""
}

# 运行所有测试（串行执行每个测试）
run_all_tests() {
    local test_names=(
        "test_auth_logic_flow"
        "test_auth_validation"
        "test_workspace_validation"
        "test_workspace_logic_operations"
        "test_user_crud"
        "test_workspace_crud_and_query"
        "test_list_workspaces_logic"
    )
    
    local passed_count=0
    local failed_count=0
    local failed_tests=()
    
    echo -e "${CYAN}串行执行所有测试...${NC}"
    echo ""
    
    for test_name in "${test_names[@]}"; do
        echo -e "${YELLOW}运行: $test_name${NC}"
        
        local output_flags=""
        if [ $SHOW_OUTPUT -eq 1 ]; then
            output_flags="-- --show-output"
        elif [ $NO_CAPTURE -eq 1 ]; then
            output_flags="-- --nocapture"
        fi
        
        local cmd="cargo test --package crv-hive --lib $test_name $output_flags"
        
        if eval "$cmd"; then
            echo -e "  ${GREEN}✓ $test_name 通过${NC}"
            ((passed_count++))
        else
            echo -e "  ${RED}✗ $test_name 失败${NC}"
            ((failed_count++))
            failed_tests+=("$test_name")
        fi
        echo ""
    done
    
    echo -e "${CYAN}=====================================${NC}"
    echo -e "测试汇总:"
    echo -e "  ${GREEN}通过: $passed_count${NC}"
    echo -e "  ${RED}失败: $failed_count${NC}"
    
    if [ $failed_count -gt 0 ]; then
        echo ""
        echo -e "${RED}失败的测试:${NC}"
        for test in "${failed_tests[@]}"; do
            echo -e "  ${RED}- $test${NC}"
        done
        return 1
    fi
    
    return 0
}

# 主函数
main() {
    echo -e "${CYAN}=====================================${NC}"
    echo -e "${CYAN}  Chronoverse 测试启动脚本${NC}"
    echo -e "${CYAN}=====================================${NC}"
    echo ""
    
    # 检查是否在项目根目录
    if [ ! -f "Cargo.toml" ]; then
        echo -e "${RED}错误: 请在项目根目录运行此脚本${NC}"
        exit 1
    fi
    
    # 检查 MongoDB
    if ! check_mongodb; then
        echo ""
        read -p "是否继续运行测试? (测试可能会失败) [y/N] " response
        if [[ ! "$response" =~ ^[Yy]$ ]]; then
            echo -e "${YELLOW}测试已取消${NC}"
            exit 0
        fi
    fi
    
    echo ""
    show_test_info "$TEST_TYPE"
    
    # 设置环境变量
    if [ $VERBOSE -eq 1 ]; then
        export RUST_BACKTRACE=1
    fi
    
    # 如果是运行所有测试，使用串行执行
    if [ "$TEST_TYPE" = "all" ]; then
        echo -e "${CYAN}执行模式: 串行执行每个测试（避免缓存冲突）${NC}"
        echo ""
        echo -e "${CYAN}=====================================${NC}"
        echo ""
        
        if run_all_tests; then
            echo ""
            echo -e "${CYAN}=====================================${NC}"
            echo -e "${GREEN}✓ 所有测试通过${NC}"
            exit 0
        else
            echo ""
            echo -e "${CYAN}=====================================${NC}"
            echo -e "${RED}✗ 部分测试失败${NC}"
            exit 1
        fi
    else
        # 运行特定类型的测试
        test_cmd=$(get_test_command "$TEST_TYPE")
        
        echo -e "执行命令: ${CYAN}$test_cmd${NC}"
        echo ""
        echo -e "${CYAN}=====================================${NC}"
        echo ""
        
        # 运行测试
        if eval "$test_cmd"; then
            echo ""
            echo -e "${CYAN}=====================================${NC}"
            echo -e "${GREEN}✓ 测试通过${NC}"
            exit 0
        else
            echo ""
            echo -e "${CYAN}=====================================${NC}"
            echo -e "${RED}✗ 测试失败${NC}"
            exit 1
        fi
    fi
}

# 运行主函数
main

