use basic_system::system_functions::bn254_ecadd::Bn254AddImpl;
use basic_system::system_functions::sha256::Sha256Impl;
use zk_ee::reference_implementations::BaseResources;
use zk_ee::system::SystemFunction;
use zk_ee::system::Resource;
use zk_ee::reference_implementations::DecreasingNative;
use zk_ee::system::errors::subsystem::SubsystemError;
use zk_ee::system::base_system_functions::{Bn254AddErrors,Sha256Errors};
use core::slice::SlicePattern;
use crypto::init_lib;


pub fn ecadd(src: &[u8], dst: &mut Vec<u8>) -> Result<(), SubsystemError<Bn254AddErrors>> {
    crypto::init_lib();
    let allocator = std::alloc::Global;
    let mut resource = <BaseResources<DecreasingNative> as Resource>::FORMAL_INFINITE;
    Bn254AddImpl::execute(&src.as_slice(), dst, &mut resource, allocator)
}

pub fn sha256(src: &[u8], dst: &mut Vec<u8>) -> Result<(), SubsystemError<Sha256Errors>> {
    crypto::init_lib();
    let allocator = std::alloc::Global;
    let mut resource = <BaseResources<DecreasingNative> as Resource>::FORMAL_INFINITE;
    Sha256Impl::execute(&src.as_slice(), dst, &mut resource, allocator)
}