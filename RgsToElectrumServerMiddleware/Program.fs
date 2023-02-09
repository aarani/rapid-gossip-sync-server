open System
open System.Threading.Tasks
open Microsoft.AspNetCore.Builder
open Microsoft.Extensions.Hosting

open GWallet.Backend
open GWallet.Backend.UtxoCoin

[<EntryPoint>]
let main args =
    let builder = WebApplication.CreateBuilder args
    let app = builder.Build()

    app.Urls.Add "http://0.0.0.0:5108"

    app.MapGet("/getTransaction/{height}/{txPos}",
        Func<string, string, Task<string>>(
            fun height txPos ->
                async {
                    Console.WriteLine(sprintf "Request: looking for transaction #%s at block height #%s" txPos height)

                    let height = Convert.ToUInt32 height
                    let txPos = Convert.ToUInt32 txPos

                    let querySettings =
                        QuerySettings.Default ServerSelectionMode.Fast

                    let getTransactionFromPosIdJob = ElectrumClient.GetBlockchainTransactionIdFromPos height txPos
                    let! txId =
                        Server.Query Currency.BTC querySettings getTransactionFromPosIdJob None

                    let getTransactionJob = ElectrumClient.GetBlockchainTransaction txId
                    let! tx =
                        Server.Query Currency.BTC querySettings getTransactionJob None

                    return tx
                } |> Async.StartAsTask
        )
    ) |> ignore

    app.Run()

    0 // Exit code

