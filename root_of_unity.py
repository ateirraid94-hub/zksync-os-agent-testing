import math

FIELD_ELEMENTS_PER_EXT_BLOB = 8192
PRIMITIVE_ROOT_OF_UNITY = 7
BLS_MODULUS = 52435875175126190479447740508185965837690552500527637822603658699938581184513

order = int(math.log2(FIELD_ELEMENTS_PER_EXT_BLOB))
root_of_unity = pow(PRIMITIVE_ROOT_OF_UNITY, (BLS_MODULUS - 1) // (2**order), BLS_MODULUS)
uint64s = [(root_of_unity >> (64 * i)) & 0xFFFFFFFFFFFFFFFF for i in range(4)]
values = [f"0x{uint64:016x}" for uint64 in uint64s]
print(f"{{{', '.join(values)}}}")