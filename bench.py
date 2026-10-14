import asyncio
import time
import json
from pathlib import Path

# Create dummy URL file
runtime_dir = Path("/tmp/screentime_bench")
runtime_dir.mkdir(exist_ok=True)
url_file = runtime_dir / "chrome_url"
# Create a fairly large payload to simulate IO/parsing overhead
url_file.write_text(json.dumps({"url": "https://example.com/some/long/url", "dummy_data": "x" * 1024 * 1024 * 50})) # 50MB file

def read_sync():
    try:
        if not url_file.exists(): return None
        with open(url_file) as f:
            data = json.load(f)
        return data.get("url")
    except Exception:
        return None

async def read_async():
    def _read_and_parse():
        try:
            if not url_file.exists(): return None
            with open(url_file) as f:
                data = json.load(f)
            return data.get("url")
        except Exception:
            return None
    return await asyncio.to_thread(_read_and_parse)

async def measure_single_call_blocking(func_to_test):
    delays = []

    async def ping_task():
        for _ in range(20):
            start = time.perf_counter()
            await asyncio.sleep(0.01)
            delays.append(time.perf_counter() - start - 0.01)

    bg = asyncio.create_task(ping_task())
    await asyncio.sleep(0.005)

    for _ in range(3):
        start = time.perf_counter()
        if asyncio.iscoroutinefunction(func_to_test):
            await func_to_test()
        else:
            func_to_test()
        end = time.perf_counter()
        await asyncio.sleep(0.05) # yield so ping_task can run between calls

    await bg
    return max(delays)

async def main():
    print("Testing sync (blocking)...")
    sync_max_delay = await measure_single_call_blocking(read_sync)
    print(f"Sync: Max Event Loop Blocked Time: {sync_max_delay:.4f}s")

    print("Testing async (non-blocking)...")
    async_max_delay = await measure_single_call_blocking(read_async)
    print(f"Async: Max Event Loop Blocked Time: {async_max_delay:.4f}s")

if __name__ == "__main__":
    asyncio.run(main())
