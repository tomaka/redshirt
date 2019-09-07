fn main() {
    async_std::task::block_on(kernel_lib::run());
}
