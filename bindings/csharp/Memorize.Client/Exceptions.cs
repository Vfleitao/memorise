using Grpc.Core;

namespace Memorize.Client;

/// <summary>
/// Exception thrown when the Memorize server's storage capacity is exceeded.
/// </summary>
public class StorageFullException : Exception
{
    /// <summary>
    /// The underlying gRPC status code (ResourceExhausted).
    /// </summary>
    public StatusCode StatusCode { get; }

    /// <summary>
    /// Creates a new StorageFullException.
    /// </summary>
    public StorageFullException(string message) : base(message)
    {
        StatusCode = StatusCode.ResourceExhausted;
    }

    /// <summary>
    /// Creates a new StorageFullException from an RpcException.
    /// </summary>
    public StorageFullException(string message, RpcException innerException) 
        : base(message, innerException)
    {
        StatusCode = innerException.StatusCode;
    }
}

/// <summary>
/// Extension methods for handling Memorize exceptions.
/// </summary>
public static class MemorizeExceptionExtensions
{
    /// <summary>
    /// Checks if an RpcException indicates the server storage is full.
    /// </summary>
    public static bool IsStorageFull(this RpcException ex)
    {
        return ex.StatusCode == StatusCode.ResourceExhausted;
    }

    /// <summary>
    /// Converts an RpcException to a StorageFullException if applicable.
    /// </summary>
    public static StorageFullException? AsStorageFullException(this RpcException ex)
    {
        if (ex.IsStorageFull())
        {
            return new StorageFullException(ex.Status.Detail, ex);
        }
        return null;
    }
}
