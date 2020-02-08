wget https://raw.githubusercontent.com/webgpu-native/webgpu-headers/master/webgpu.h
bindgen --use-core --ctypes-prefix ::libc webgpu.h &> ./src/bindings.rs
rm webgpu.h
