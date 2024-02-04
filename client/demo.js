(async () => {
    console.log(
        "%cWASM is significantly slower with DevTools open!",
        "color:red;font-size:3rem;font-weight:bold"
    );

    const params = (new URL(window.location.href)).searchParams;

    const should_feature_test = params.has("feature_test");
    const should_multiparallel_test = params.has("multi_parallel_test");
    const should_parallel_test = params.has("parallel_test");
    const should_multiperf_test = params.has("multi_perf_test");
    const should_perf_test = params.has("perf_test");
    const should_ws_test = params.has("ws_test");

    const log = (str) => {
        let el = document.createElement("div");
        el.innerText = str;
        document.getElementById("logs").appendChild(el);
        console.warn(str);
        window.scrollTo(0, document.body.scrollHeight);
    }

    let { EpoxyClient } = await epoxy();

    const tconn0 = performance.now();
    // args: websocket url, user agent, redirect limit 
    let epoxy_client = await new EpoxyClient("wss://localhost:4000", navigator.userAgent, 10);
    const tconn1 = performance.now();
    log(`conn establish took ${tconn1 - tconn0} ms or ${(tconn1 - tconn0) / 1000} s`);

    const test_mux = async (url) => {
        const t0 = performance.now();
        await epoxy_client.fetch(url);
        const t1 = performance.now();
        return t1 - t0;
    };

    const test_native = async (url) => {
        const t0 = performance.now();
        await fetch(url, { cache: "no-store" });
        const t1 = performance.now();
        return t1 - t0;
    };

    if (should_feature_test) {
        for (const url of [
            ["https://httpbin.org/get", {}],
            ["https://httpbin.org/gzip", {}],
            ["https://httpbin.org/brotli", {}],
            ["https://httpbin.org/redirect/11", {}],
            ["https://httpbin.org/redirect/1", { redirect: "manual" }]
        ]) {
            let resp = await epoxy_client.fetch(url[0], url[1]);
            console.warn(url, resp, Object.fromEntries(resp.headers));
            console.warn(await resp.text());
        }
    } else if (should_multiparallel_test) {
        const num_tests = 10;
        let total_mux_minus_native = 0;
        for (const _ of Array(num_tests).keys()) {
            let total_mux = 0;
            await Promise.all([...Array(num_tests).keys()].map(async i => {
                log(`running mux test ${i}`);
                return await test_mux("https://httpbin.org/get");
            })).then((vals) => { total_mux = vals.reduce((acc, x) => acc + x, 0) });
            total_mux = total_mux / num_tests;

            let total_native = 0;
            await Promise.all([...Array(num_tests).keys()].map(async i => {
                log(`running native test ${i}`);
                return await test_native("https://httpbin.org/get");
            })).then((vals) => { total_native = vals.reduce((acc, x) => acc + x, 0) });
            total_native = total_native / num_tests;

            log(`avg mux (${num_tests}) took ${total_mux} ms or ${total_mux / 1000} s`);
            log(`avg native (${num_tests}) took ${total_native} ms or ${total_native / 1000} s`);
            log(`avg mux - avg native (${num_tests}): ${total_mux - total_native} ms or ${(total_mux - total_native) / 1000} s`);
            total_mux_minus_native += total_mux - total_native;
        }
        total_mux_minus_native = total_mux_minus_native / num_tests;
        log(`total mux - native (${num_tests} tests of ${num_tests} reqs): ${total_mux_minus_native} ms or ${total_mux_minus_native / 1000} s`);
    } else if (should_parallel_test) {
        const num_tests = 10;

        let total_mux = 0;
        await Promise.all([...Array(num_tests).keys()].map(async i => {
            log(`running mux test ${i}`);
            return await test_mux("https://httpbin.org/get");
        })).then((vals) => { total_mux = vals.reduce((acc, x) => acc + x, 0) });
        total_mux = total_mux / num_tests;

        let total_native = 0;
        await Promise.all([...Array(num_tests).keys()].map(async i => {
            log(`running native test ${i}`);
            return await test_native("https://httpbin.org/get");
        })).then((vals) => { total_native = vals.reduce((acc, x) => acc + x, 0) });
        total_native = total_native / num_tests;

        log(`avg mux (${num_tests}) took ${total_mux} ms or ${total_mux / 1000} s`);
        log(`avg native (${num_tests}) took ${total_native} ms or ${total_native / 1000} s`);
        log(`avg mux - avg native (${num_tests}): ${total_mux - total_native} ms or ${(total_mux - total_native) / 1000} s`);
    } else if (should_multiperf_test) {
        const num_tests = 10;
        let total_mux_minus_native = 0;
        for (const _ of Array(num_tests).keys()) {
            let total_mux = 0;
            for (const i of Array(num_tests).keys()) {
                log(`running mux test ${i}`);
                total_mux += await test_mux("https://httpbin.org/get");
            }
            total_mux = total_mux / num_tests;

            let total_native = 0;
            for (const i of Array(num_tests).keys()) {
                log(`running native test ${i}`);
                total_native += await test_native("https://httpbin.org/get");
            }
            total_native = total_native / num_tests;

            log(`avg mux (${num_tests}) took ${total_mux} ms or ${total_mux / 1000} s`);
            log(`avg native (${num_tests}) took ${total_native} ms or ${total_native / 1000} s`);
            log(`avg mux - avg native (${num_tests}): ${total_mux - total_native} ms or ${(total_mux - total_native) / 1000} s`);
            total_mux_minus_native += total_mux - total_native;
        }
        total_mux_minus_native = total_mux_minus_native / num_tests;
        log(`total mux - native (${num_tests} tests of ${num_tests} reqs): ${total_mux_minus_native} ms or ${total_mux_minus_native / 1000} s`);

    } else if (should_perf_test) {
        const num_tests = 10;

        let total_mux = 0;
        for (const i of Array(num_tests).keys()) {
            log(`running mux test ${i}`);
            total_mux += await test_mux("https://httpbin.org/get");
        }
        total_mux = total_mux / num_tests;

        let total_native = 0;
        for (const i of Array(num_tests).keys()) {
            log(`running native test ${i}`);
            total_native += await test_native("https://httpbin.org/get");
        }
        total_native = total_native / num_tests;

        log(`avg mux (${num_tests}) took ${total_mux} ms or ${total_mux / 1000} s`);
        log(`avg native (${num_tests}) took ${total_native} ms or ${total_native / 1000} s`);
        log(`avg mux - avg native (${num_tests}): ${total_mux - total_native} ms or ${(total_mux - total_native) / 1000} s`);
    } else if (should_ws_test) {
        let ws = await epoxy_client.connect_ws(
            () => console.log("opened"),
            () => console.log("closed"),
            err => console.error(err),
            msg => console.log(msg),
            "wss://echo.websocket.events",
            [],
            "localhost"
        );
        while (true) {
            await ws.send("data");
            await (new Promise((res, _) => setTimeout(res, 100)));
        }
    } else {
        let resp = await epoxy_client.fetch("https://httpbin.org/get");
        console.warn(resp, Object.fromEntries(resp.headers));
        console.warn(await resp.text());
    }
    log("done");
})();
