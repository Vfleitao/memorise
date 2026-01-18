using BenchmarkDotNet.Running;
using BenchmarkDotNet.Configs;
using BenchmarkDotNet.Jobs;
using BenchmarkDotNet.Columns;
using BenchmarkDotNet.Reports;
using Memorize.Benchmarks;

// Configure benchmark
var config = DefaultConfig.Instance
    .WithSummaryStyle(SummaryStyle.Default.WithRatioStyle(BenchmarkDotNet.Columns.RatioStyle.Trend))
    .AddColumn(StatisticColumn.P95)
    .AddColumn(StatisticColumn.OperationsPerSecond);

Console.WriteLine("╔═══════════════════════════════════════════════════════════╗");
Console.WriteLine("║          Memorize vs Redis Benchmarks                     ║");
Console.WriteLine("╚═══════════════════════════════════════════════════════════╝");
Console.WriteLine();

// Show usage
Console.WriteLine("Usage: dotnet run -c Release -- [options]");
Console.WriteLine();
Console.WriteLine("Options:");
Console.WriteLine("  --quick           Run short benchmarks (fewer iterations)");
Console.WriteLine("  --memorize        Run only Memorize benchmarks");
Console.WriteLine("  --redis           Run only Redis benchmarks");
Console.WriteLine("  --all             Run both Memorize and Redis benchmarks (default)");
Console.WriteLine();

var serverUrl = Environment.GetEnvironmentVariable("MEMORIZE_URL") ?? "http://localhost:50051";
var redisHost = Environment.GetEnvironmentVariable("REDIS_HOST") ?? "localhost:6379";
Console.WriteLine($"Memorize server: {serverUrl}");
Console.WriteLine($"Redis server: {redisHost}");
Console.WriteLine();

// Parse args
bool quick = args.Contains("--quick");
bool memorizeOnly = args.Contains("--memorize");
bool redisOnly = args.Contains("--redis");
bool runAll = args.Contains("--all") || (!memorizeOnly && !redisOnly);

// Build config
IConfig runConfig;
if (quick)
{
    runConfig = new ManualConfig()
        .AddExporter(BenchmarkDotNet.Exporters.MarkdownExporter.GitHub)
        .AddLogger(BenchmarkDotNet.Loggers.ConsoleLogger.Default)
        .AddColumnProvider(DefaultColumnProviders.Instance)
        .WithOptions(ConfigOptions.DisableOptimizationsValidator)
        .AddJob(Job.ShortRun);
}
else
{
    runConfig = config;
}

// Run benchmarks
if (runAll)
{
    Console.WriteLine("Running both Memorize and Redis benchmarks...");
    Console.WriteLine("Make sure both servers are running!");
    Console.WriteLine();
    BenchmarkRunner.Run(new[] { typeof(MemorizeBenchmarks), typeof(RedisBenchmarks) }, runConfig);
}
else if (memorizeOnly)
{
    Console.WriteLine("Running Memorize benchmarks only...");
    Console.WriteLine("Make sure Memorize server is running!");
    Console.WriteLine();
    BenchmarkRunner.Run<MemorizeBenchmarks>(runConfig);
}
else if (redisOnly)
{
    Console.WriteLine("Running Redis benchmarks only...");
    Console.WriteLine("Make sure Redis server is running!");
    Console.WriteLine();
    BenchmarkRunner.Run<RedisBenchmarks>(runConfig);
}
