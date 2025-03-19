enum FiberValueType {
    VOID = <u8>0,
    I32 = <u8>1,
    I64 = <u8>2,
    F32 = <u8>3,
    F64 = <u8>4,
    LIST = <u8>5
}

@unmanaged
class FiberValue {
    type: FiberValueType = FiberValueType.VOID;
    private _value: i64 = 0;

    get asI32(): i32 {
        return this._value as i32;
    }

    get asI64(): i64 {
        return this._value;
    }

    get asF32(): f32 {
        return reinterpret<f32>(i32(this._value));
    }

    get asF64(): f64 {
        return reinterpret<f64>(this._value);
    }

    get asListPtr(): usize {
        return this._value as usize;
    }

    static fromI32(value: i32): FiberValue {
        const fv = new FiberValue();
        fv.type = FiberValueType.I32;
        fv._value = value;
        return fv;
    }

    static fromI64(value: i64): FiberValue {
        const fv = new FiberValue();
        fv.type = FiberValueType.I64;
        fv._value = value;
        return fv;
    }

    static fromF32(value: f32): FiberValue {
        const fv = new FiberValue();
        fv.type = FiberValueType.F32;
        fv._value = reinterpret<i32>(value);
        return fv;
    }

    static fromF64(value: f64): FiberValue {
        const fv = new FiberValue();
        fv.type = FiberValueType.F64;
        fv._value = reinterpret<i64>(value);
        return fv;
    }

    static fromListPtr(ptr: usize): FiberValue {
        const fv = new FiberValue();
        fv.type = FiberValueType.LIST;
        fv._value = ptr as i64;
        return fv;
    }
}

// Opaque handle types
@unmanaged
class StateHandle {}

@unmanaged
class Scope {}

// State
@external("env", "use_state")
declare function use_state(value: FiberValue): StateHandle;

@external("env", "state_get")
declare function state_get(handle: StateHandle): FiberValue;

@external("env", "state_get_raw")
declare function state_get_raw(handle: StateHandle): FiberValue;

@external("env", "state_set")
declare function state_set(handle: StateHandle, value: FiberValue): void;

// Effect
@external("env", "use_effect")
declare function use_effect(cx: usize, callback: (cx: usize) => void): void;

// Scope
@external("env", "create_root")
declare function create_root(callback: () => void): Scope;

@unmanaged
class ClosureContext {
    input: StateHandle = changetype<StateHandle>(0);
    squared: StateHandle = changetype<StateHandle>(0);
}

function effectCallback(ctx: usize): void {
    const context = changetype<ClosureContext>(ctx);
    const value = state_get(context.input);
    if (value.type === FiberValueType.I32) {
        const v = value.asI32;
        state_set(context.squared, FiberValue.fromI32(v * v));
    }
}

export function run(): Uint8ClampedArray {
    let result = new Uint8ClampedArray(4);

    const input = use_state(FiberValue.fromI32(0));
    const squared = use_state(FiberValue.fromI32(0));

    const context = new ClosureContext();
    context.input = input;
    context.squared = squared;

    use_effect(changetype<usize>(context), changetype<(ctx: usize) => void>(effectCallback));
    
    result[0] = state_get(input).asI32;

    state_set(input, FiberValue.fromI32(2));
    result[1] = state_get(squared).asI32;

    state_set(input, FiberValue.fromI32(5));
    result[2] = state_get(squared).asI32;

    state_set(input, FiberValue.fromI32(-3));
    result[3] = state_get(squared).asI32;

    return result;
}